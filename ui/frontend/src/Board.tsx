import { useState } from "react";

import { api } from "./api";
import { blockedByBoard, boardNames, byStage, columnsFor, DEFAULT_BOARD, isBlocked, isWorking, itemKey, KIND_ICONS, treeOrder, UNKNOWN_STAGE } from "./boardState";
import { LinkBadges } from "./LinkBadges";
import { ReviewStrip } from "./ReviewStrip";
import { IconInfo, IconTrash } from "./icons";
import { StageSource } from "./StageSource";
import { Item, Pipelines } from "./types";
import { useRun } from "./useRun";

type Props = {
  items: Item[];
  /** Boards of every project; `projects` are the ones on screen. */
  pipelines: Pipelines | undefined;
  projects: string[];
  /** Board asked for by the URL; falls back to the first one that exists. */
  board: string | undefined;
  onBoard: (name: string) => void;
  selected: string | null;
  onSelect: (item: Item) => void;
  showProject: boolean;
  /** Called after items were removed. */
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

function Card({ item, selected, onSelect, showProject, depth = 0 }: { item: Item; selected: boolean; onSelect: () => void; showProject: boolean; depth?: number }) {
  const icon = KIND_ICONS[item.kind.trim().toLowerCase()];
  return (
    <div
      style={depth ? { marginLeft: depth * 16 } : undefined}
      className={`card${selected ? " selected" : ""}${isWorking(item) ? " working" : ""}${isBlocked(item) ? " blocked" : ""}`}
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
        {showProject && <span className="badge">{item.project}</span>}
        <span className="badge">{item.kind}</span>
        <LinkBadges item={item} />
      </div>
      {(item.review || isBlocked(item) || icon || !!item.unpushed || item.pr_moved) && (
        <div className="card-foot">
          <div className="foot-left">
            <ReviewStrip review={item.review} />
            {isBlocked(item) && <div className="chip blocked">blocked</div>}
            {!!item.unpushed && <div className="chip unpushed" title="Commits in the worktree that are not on any remote">{item.unpushed} unpushed</div>}
            {item.pr_moved && <div className="chip pr-moved" title="The PR has commits this workitem has not seen">new commits</div>}
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

export function Board({ items, pipelines, projects, board, onBoard, selected, onSelect, showProject, onChanged }: Props) {
  const names = boardNames(pipelines, projects);
  const active = board && names.includes(board) ? board : (names[0] ?? DEFAULT_BOARD);
  const columns = byStage(items, pipelines, active);
  const shown = columnsFor(pipelines, projects, active);
  if (pipelines && columns[UNKNOWN_STAGE].length) shown.push({ name: UNKNOWN_STAGE, label: UNKNOWN_STAGE, final: false });
  const blocked = blockedByBoard(items, pipelines);
  const broken = projects.flatMap((name) => {
    const found = pipelines?.[name]?.find((b) => b.name === active);
    return found?.error ? [{ project: name, pipeline: found.pipeline, error: found.error }] : [];
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
    <Card key={itemKey(item)} item={item} depth={depth} selected={selected === itemKey(item)} onSelect={() => onSelect(item)} showProject={showProject} />
  );
  return (
    <>
      <nav className="board-tabs" role="tablist">
        {names.length > 1 &&
          names.map((name) => (
            <button key={name} role="tab" aria-selected={name === active} className={name === active ? "active" : ""} onClick={() => onBoard(name)}>
              {name}
              {blocked[name] ? <span className="count blocked" title="blocked items">{blocked[name]}</span> : null}
            </button>
          ))}
        <div className="view-toggle" role="group" aria-label="View">
          {(["board", "list"] as const).map((v) => (
            <button key={v} aria-pressed={view === v} className={view === v ? "active" : ""} onClick={() => chooseView(v)}>
              {v}
            </button>
          ))}
        </div>
      </nav>
      {broken.map(({ project, pipeline, error }) => (
        <p className="error" key={project}>
          {project}: board "{active}" file not in use, {pipeline ? "showing the last valid one" : "no valid board yet"}. {error}
        </p>
      ))}
      <div className={`board${view === "list" ? " list" : ""}`}>
        {shown.map(({ name, label, final }) => {
          const inColumn = columns[name] ?? [];
          return (
            <section className={`column${name === UNKNOWN_STAGE ? " unknown" : ""}`} data-stage={name} key={name}>
              <h2>
                {label} <span className="count">{inColumn.length}</span>
                {name !== UNKNOWN_STAGE && (
                  <button className="link info" title="What this stage runs" aria-label={`What ${label} runs`} onClick={() => setInspected({ name, label })}>
                    <IconInfo />
                  </button>
                )}
                {final && inColumn.length > 0 && <ClearColumn items={inColumn} onDone={onChanged} />}
              </h2>
              <div className="cards">{treeOrder(inColumn).map(({ item, depth }) => card(item, depth))}</div>
            </section>
          );
        })}
      </div>
      {inspected && <StageSource {...inspected} stage={inspected.name} board={active} projects={projects} onClose={() => setInspected(undefined)} />}
    </>
  );
}
