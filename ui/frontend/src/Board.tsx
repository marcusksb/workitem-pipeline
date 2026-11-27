import { FormEvent, ReactNode, useState } from "react";

import { api } from "./api";
import { AgentStatus } from "./AgentStatus";
import { boardNames, boardProblems, byStage, cardChildren, stageTitle, columnsFor, isBlocked, isWaiting, isWorking, itemKey, KIND_ICONS, needsApproval, boardLabel, openDependencies, orderFor, pauseState, stageProblems, treeOrder, UNKNOWN_STAGE } from "./boardState";
import { BoardProblemsPanel } from "./BoardProblems";
import { Choice, Dialog, DialogForm } from "./Dialog";
import { FollowUpsChip } from "./FollowUpsChip";
import { LinkBadges } from "./LinkBadges";
import { PrStatus } from "./PrStatus";
import { IconAlert, IconPause, IconPlay, IconSettings, IconTrash } from "./icons";
import { ErrorText, Muted } from "./parts";
import { StageSource } from "./StageSource";
import { STAGE_DOT } from "./tone";
import type { View } from "./TopBar";
import { AgentSession, Item, Pipelines } from "./types";
import { Tab } from "./url";
import { useRun } from "./useRun";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

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
  /** Agent sessions without a workitem; a running board fix replaces the fix button. */
  sessions: AgentSession[] | undefined;
  /** Called after items were removed or a column was paused. */
  onChanged: () => void;
};

type Runner = ReturnType<typeof useRun>;

/** Removes the items of a final column, one by one, the same way as "Remove workitem". A dialog
 * picks which: by default only those without open follow-ups, so the ones with work left stay. */
function ClearColumn({ items, runner: { busy, run } }: { items: Item[]; runner: Runner }) {
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
      <Button
        variant="ghost"
        size="icon"
        className="size-6"
        disabled={busy}
        title="Remove workitems in this column"
        aria-label="Clear"
        onClick={() => {
          setAll(false);
          setOpen(true);
        }}
      >
        <IconTrash />
      </Button>
      {open && (
        <Dialog title="Clear column" onClose={() => setOpen(false)}>
          <DialogForm onSubmit={submit}>
            <Choice>
              <input type="radio" name="clear" checked={!all} onChange={() => setAll(false)} />
              Only workitems without follow-ups ({withoutFollowUps.length})
            </Choice>
            <Choice>
              <input type="radio" name="clear" checked={all} onChange={() => setAll(true)} />
              All workitems ({items.length})
            </Choice>
            <Muted>
              Their agent sessions are stopped, and their worktrees and workitem folders are deleted. This cannot be recovered. Branches and PRs stay.
            </Muted>
            <Button type="submit" variant="danger" disabled={chosen.length === 0}>
              Remove {chosen.length} workitem{chosen.length === 1 ? "" : "s"}
            </Button>
          </DialogForm>
        </Dialog>
      )}
    </>
  );
}

/** Pauses or resumes the watchers of a column in every workspace that has them. Running scripts and sessions go on. */
function PauseColumn({ workspaces, board, stage, label, paused, runner: { busy, run } }: { workspaces: string[]; board: string; stage: string; label: string; paused: boolean; runner: Runner }) {
  const toggle = () => run(paused ? "Resume" : "Pause", () => Promise.all(workspaces.map((workspace) => api.pauseStage(workspace, board, stage, !paused))));
  return (
    <Button
      variant="ghost"
      size="icon"
      className="size-6"
      disabled={busy}
      title={paused ? "Resume this column's watchers" : "Pause this column's watchers: nothing new starts here until you resume"}
      aria-label={`${paused ? "Resume" : "Pause"} ${label}`}
      onClick={toggle}
    >
      {paused ? <IconPlay /> : <IconPause />}
    </Button>
  );
}

/** How many dependencies are not merged yet; a click opens the item's Depends on tab. */
function DependencyBadge({ count, onOpen }: { count: number; onOpen: () => void }) {
  return (
    <Badge asChild className="chip waits-on cursor-pointer border-dashed hover:text-foreground">
      <button
        title={`Waits until ${count === 1 ? "1 dependency is" : `${count} dependencies are`} merged`}
        onClick={(event) => {
          event.stopPropagation();
          onOpen();
        }}
      >
        depends on {count}
      </button>
    </Badge>
  );
}

/** `where`: board and stage of a child shown under its parent outside its own column. */
function Card({ item, selected, onSelect, showWorkspace, waitsOn, onDependencies, onParent, where, depth = 0 }: { item: Item; selected: boolean; onSelect: () => void; showWorkspace: boolean; waitsOn: Item[]; onDependencies: () => void; onParent?: () => void; where?: string; depth?: number }) {
  const icon = KIND_ICONS[item.kind.trim().toLowerCase()];
  return (
    <div
      style={depth ? { marginLeft: depth * 16 } : undefined}
      className={cn(
        "card flex w-full cursor-pointer flex-col gap-2 rounded-lg border border-border bg-card px-3 py-2.5 text-left shadow-[0_1px_2px_rgb(16_24_40/0.04)] outline-none hover:border-primary/35 focus-visible:ring-[3px] focus-visible:ring-ring/30",
        "group-data-[view=list]/board:flex-row group-data-[view=list]/board:flex-wrap group-data-[view=list]/board:items-center group-data-[view=list]/board:gap-x-3 group-data-[view=list]/board:gap-y-1 group-data-[view=list]/board:py-2",
        depth > 0 && "w-auto border-l-[3px]",
        isBlocked(item) ? "blocked border-blocked/75 inset-ring inset-ring-blocked/75 bg-[color-mix(in_srgb,var(--blocked)_3%,var(--card))]" : isWaiting(item) ? "waiting border-waiting/75 inset-ring inset-ring-waiting/75" : needsApproval(item) && "border-gate/35 inset-ring inset-ring-gate/35",
        isWorking(item) && "working",
        selected && "selected bg-[color-mix(in_srgb,var(--primary)_12%,var(--card))]",
      )}
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
      <div className="text-[15.5px] leading-[1.4] font-medium [overflow-wrap:anywhere] group-data-[view=list]/board:order-1 group-data-[view=list]/board:min-w-0 group-data-[view=list]/board:flex-[1_1_240px]">
        {icon && <span className="mr-1.5 text-sm leading-none" title={item.kind}>{icon}</span>}
        {item.title}
      </div>
      <div className="flex min-w-0 flex-wrap gap-1 *:max-w-full group-data-[view=list]/board:order-2">
        <Badge>{item.kind}</Badge>
        {showWorkspace && (
          <Badge title={item.workspace}>
            <span className="truncate">{item.workspace}</span>
          </Badge>
        )}
        {where && (
          <Badge title={where}>
            <span className="truncate">{where}</span>
          </Badge>
        )}
        <LinkBadges item={item} pr={false} onParent={onParent} />
        {waitsOn.length > 0 && <DependencyBadge count={waitsOn.length} onOpen={onDependencies} />}
        <FollowUpsChip followUps={item.follow_ups} />
        {!!item.unpushed && (
          <Badge title="Commits in the worktree that are not on any remote">{item.unpushed} unpushed</Badge>
        )}
      </div>
      <div className="flex min-w-0 items-center gap-2 empty:hidden group-data-[view=list]/board:contents">
        <AgentStatus item={item} className="group-data-[view=list]/board:order-4 group-data-[view=list]/board:max-w-[320px]" />
        <PrStatus item={item} className="ml-auto flex-none group-data-[view=list]/board:order-3 group-data-[view=list]/board:ml-0" />
      </div>
    </div>
  );
}

/**
 * A stage: header (dot, name, problems, count, info, pause, clear) and its cards.
 */
function Column({ name, label, final, items, board, pause, problems, onInspect, onChanged, children }: { name: string; label: string; final: boolean; items: Item[]; board: string; pause: { workspaces: string[]; paused: boolean }; problems: string[]; onInspect: () => void; onChanged: () => void; children: ReactNode }) {
  const clear = useRun(onChanged);
  const pauser = useRun(onChanged);
  const errors = [
    ["pause", pauser.error],
    ["clear", clear.error],
  ].filter(([, error]) => error);
  return (
    <section
      className="column flex min-h-[120px] flex-col gap-1.5 rounded-[10px] bg-[color-mix(in_srgb,var(--foreground)_4%,var(--background))] p-1.5 group-data-[view=list]/board:min-h-0"
      data-stage={name}
    >
      <h2 className="sticky -top-4 z-[1] flex min-h-8 items-center gap-2 bg-inherit pl-1.5 text-[15px] font-semibold capitalize">
        <span className={cn("size-2 flex-none rounded-full", name === UNKNOWN_STAGE ? "bg-blocked" : (STAGE_DOT[name] ?? "bg-muted-foreground"))} aria-hidden="true" />
        <span className="min-w-0 truncate" title={label}>
          {label}
        </span>
        {problems.length > 0 && (
          <span className="flex-none text-blocked" role="img" title={problems.join("\n")} aria-label={`${label} problems: ${problems.join("; ")}`}>
            <IconAlert />
          </span>
        )}
        {pause.paused && <Badge>Paused</Badge>}
        <span className="text-xs font-normal text-muted-foreground">{items.length}</span>
        <span className="ml-auto flex items-center">
          {name !== UNKNOWN_STAGE && (
            <Button variant="ghost" size="icon" className="size-6" title="Stage settings" aria-label={`${label} settings`} onClick={onInspect}>
              <IconSettings />
            </Button>
          )}
          {pause.workspaces.length > 0 && <PauseColumn workspaces={pause.workspaces} board={board} stage={name} label={label} paused={pause.paused} runner={pauser} />}
          {final && items.length > 0 && <ClearColumn items={items} runner={clear} />}
        </span>
      </h2>
      <div className={cn("flex flex-col gap-1.5", pause.paused && "[&>:not(p)]:opacity-55")}>
        {errors.map(([source, error]) => (
          <ErrorText className="text-xs" key={source}>
            {error}
          </ErrorText>
        ))}
        {children}
      </div>
    </section>
  );
}

export function Board({ items, pipelines, workspaces, board, view, selected, onSelect, showWorkspace, sessions, onChanged }: Props) {
  const names = boardNames(pipelines, workspaces);
  const active = board && names.includes(board) ? board : (names[0] ?? "");
  const columns = byStage(items, pipelines, active);
  const shown = columnsFor(pipelines, workspaces, active);
  if (pipelines && columns[UNKNOWN_STAGE].length) shown.push({ name: UNKNOWN_STAGE, label: UNKNOWN_STAGE, final: false });
  const problems = boardProblems(pipelines, workspaces, active, shown);
  const [inspected, setInspected] = useState<{ name: string; label: string }>();
  const card = (item: Item, depth = 0, where?: string) => {
    const parent = item.parent ? items.find((p) => p.workspace === item.workspace && p.dir === item.parent) : undefined;
    return (
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
        onParent={parent ? () => onSelect(parent) : undefined}
      />
    );
  };
  return (
    <>
      {problems.map((found) => (
        <BoardProblemsPanel key={found.workspace} found={found} board={active} boardLabel={boardLabel(pipelines, found.workspace, active)} sessions={sessions ?? []} showWorkspace={workspaces.length > 1} />
      ))}
      <div
        data-view={view}
        className={cn("board group/board gap-2.5", view === "list" ? "list flex flex-col" : "grid auto-cols-[minmax(240px,1fr)] grid-flow-col")}
      >
        {shown.map(({ name, label, final }) => {
          const inColumn = columns[name] ?? [];
          const pause = pauseState(pipelines, workspaces, active, name);
          return (
            <Column key={name} name={name} label={label} final={final} items={inColumn} board={active} pause={pause} problems={stageProblems(pipelines, workspaces, active, name)} onInspect={() => setInspected({ name, label })} onChanged={onChanged}>
              {treeOrder(inColumn, orderFor(pipelines, workspaces, active, name)).flatMap(({ item, depth }) => [
                card(item, depth),
                ...cardChildren(item, items).map((child) => card(child, depth + 1, `${boardLabel(pipelines, child.workspace, child.board)} · ${stageTitle(child, pipelines)}`)),
              ])}
            </Column>
          );
        })}
      </div>
      {inspected && <StageSource {...inspected} stage={inspected.name} board={active} workspaces={workspaces} onClose={() => setInspected(undefined)} />}
    </>
  );
}
