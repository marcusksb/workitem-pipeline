import { FormEvent, useState } from "react";

import { api } from "./api";
import { boardNames, byStage, cardChildren, stageTitle, columnsFor, isWaiting, isBlocked, isWorking, itemKey, KIND_ICONS, boardLabel, openDependencies, orderFor, pauseState, treeOrder, UNKNOWN_STAGE, waitingFor } from "./boardState";
import { Dialog } from "./Dialog";
import { FollowUpsChip } from "./FollowUpsChip";
import { LinkBadges } from "./LinkBadges";
import { ReviewStrip } from "./ReviewStrip";
import { IconInfo, IconPause, IconPlay, IconTrash } from "./icons";
import { StageSource } from "./StageSource";
import type { View } from "./TopBar";
import { Item, Pipelines } from "./types";
import { Tab } from "./url";
import { useRun } from "./useRun";

type Props = {
  items: Item[];
  /** Boards of every workspace; `workspaces` are the ones on screen. */
  pipelines: Pipelines | undefined;
  workspaces: string[];
  /** Board asked for by the URL; falls back to the first one that exists. */
  board: string | undefined;
  view: View;
  selected: string | null;
  onSelect: (item: Item, tab?: Tab) => void;
  showWorkspace: boolean;
  /** Called after items were removed or a column was paused. */
  onChanged: () => void;
};

/** Removes the items of a final column, one by one, the same way as "Remove workitem". A dialog
 * picks which: by default only those without open follow-ups, so the ones with work left stay. */
function ClearColumn({ items, onDone }: { items: Item[]; onDone: () => void }) {
  const { busy, error, run } = useRun(onDone);
  const [open, setOpen] = useState(false);
  const [all, setAll] = useState(false);
  const withoutFollowUps = items.filter((item) => item.follow_ups.length === 0);
  const chosen = all ? items : withoutFollowUps;
  const clear = async () => {
    const problems: string[] = [];
    for (const item of chosen) {
      try {
        const { worktree_left } = await api.remove(item);
        if (worktree_left) problems.push(`${item.title}: worktree stays: ${worktree_left}`);
      } catch (e) {
        problems.push(`${item.title}: ${e instanceof Error ? e.message : String(e)}`);
      }
    }
    if (problems.length) throw new Error(problems.join("\n"));
  };
  const submit = (event: FormEvent) => {
    event.preventDefault();
    setOpen(false);
    run("Clear", clear);
  };
  return (
    <>
      <button
        className="link clear"
        disabled={busy}
        title="Remove workitems in this column"
        onClick={() => {
          setAll(false);
          setOpen(true);
        }}
      >
        <IconTrash />
        Clear
      </button>
      {error && <p className="error clear-error">{error}</p>}
      {open && (
        <Dialog title="Clear column" onClose={() => setOpen(false)}>
          <form className="dialog-form" onSubmit={submit}>
            <label className="choice">
              <input type="radio" name="clear" checked={!all} onChange={() => setAll(false)} />
              Only workitems without follow-ups ({withoutFollowUps.length})
            </label>
            <label className="choice">
              <input type="radio" name="clear" checked={all} onChange={() => setAll(true)} />
              All workitems ({items.length})
            </label>
            <span className="hint">
              Their agent sessions are stopped, and their worktrees and workitem folders are deleted. This cannot be recovered. Branches and PRs stay.
            </span>
            <button type="submit" disabled={chosen.length === 0}>
              Remove {chosen.length} workitem{chosen.length === 1 ? "" : "s"}
            </button>
          </form>
        </Dialog>
      )}
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

/** How many dependencies are not merged yet; a click opens the item's Depends on tab. */
function DependencyBadge({ count, onOpen }: { count: number; onOpen: () => void }) {
  return (
    <button
      className="chip waits-on"
      title={`Waits until ${count === 1 ? "1 dependency is" : `${count} dependencies are`} merged`}
      onClick={(event) => {
        event.stopPropagation();
        onOpen();
      }}
    >
      depends on {count}
    </button>
  );
}

/** `where`: board and stage of a child shown under its parent outside its own column. */
function Card({ item, selected, onSelect, showWorkspace, waitsOn, onDependencies, where, depth = 0 }: { item: Item; selected: boolean; onSelect: () => void; showWorkspace: boolean; waitsOn: Item[]; onDependencies: () => void; where?: string; depth?: number }) {
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
        {where && <span className="badge where">{where}</span>}
        <span className="badge">{item.kind}</span>
        <LinkBadges item={item} />
      </div>
      {waitsOn.length > 0 && (
        <div className="meta">
          <DependencyBadge count={waitsOn.length} onOpen={onDependencies} />
        </div>
      )}
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

export function Board({ items, pipelines, workspaces, board, view, selected, onSelect, showWorkspace, onChanged }: Props) {
  const names = boardNames(pipelines, workspaces);
  const active = board && names.includes(board) ? board : (names[0] ?? "");
  const columns = byStage(items, pipelines, active);
  const shown = columnsFor(pipelines, workspaces, active);
  if (pipelines && columns[UNKNOWN_STAGE].length) shown.push({ name: UNKNOWN_STAGE, label: UNKNOWN_STAGE, final: false });
  const broken = workspaces.flatMap((name) => {
    const found = pipelines?.[name]?.find((b) => b.name === active);
    return found?.error ? [{ workspace: name, pipeline: found.pipeline, error: found.error }] : [];
  });
  const [inspected, setInspected] = useState<{ name: string; label: string }>();
  const card = (item: Item, depth = 0, where?: string) => (
    <Card
      key={itemKey(item)}
      item={item}
      depth={depth}
      where={where}
      selected={selected === itemKey(item)}
      onSelect={() => onSelect(item)}
      showWorkspace={showWorkspace}
      waitsOn={openDependencies(item, items, pipelines)}
      onDependencies={() => onSelect(item, "dependencies")}
    />
  );
  return (
    <>
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
              <div className="cards">{treeOrder(inColumn, orderFor(pipelines, workspaces, active, name)).flatMap(({ item, depth }) => [
                  card(item, depth),
                  ...cardChildren(item, items).map((child) => card(child, depth + 1, `${boardLabel(pipelines, child.workspace, child.board)} · ${stageTitle(child, pipelines)}`)),
                ])}</div>
            </section>
          );
        })}
      </div>
      {inspected && <StageSource {...inspected} stage={inspected.name} board={active} workspaces={workspaces} onClose={() => setInspected(undefined)} />}
    </>
  );
}
