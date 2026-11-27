import { ReactNode, useEffect, useRef, useState } from "react";
import { api, itemTerminal } from "./api";
import { AgentStatus } from "./AgentStatus";
import { boardLabel, boardOfWorkspace, isBlocked, KIND_ICONS, openDependencies, shownBoard, stageLabel, stageTitle, stripFrontmatter } from "./boardState";
import { Gate, Item, Pipelines } from "./types";
import { AgentActions } from "./AgentActions";
import { BriefEditor } from "./BriefEditor";
import { BriefView } from "./BriefView";
import { Children } from "./Children";
import { Dependencies } from "./Dependencies";
import { PrActions } from "./PrActions";
import { PrConversation } from "./PrConversation";
import { PrStatus } from "./PrStatus";
import { StageActions } from "./StageActions";
import { Files } from "./Files";
import { FollowUpsChip } from "./FollowUpsChip";
import { Info } from "./Info";
import { IssueBadge } from "./LinkBadges";
import { ActionBar, ErrorText, Muted, prose, scroll, SidePanel } from "./parts";
import { STAGE_DOT } from "./tone";
import { Tab } from "./url";
import { Terminal } from "./Terminal";
import { usePolling } from "./usePolling";
import { useRun } from "./useRun";
import {
  IconBoard,
  IconBot,
  IconCheck,
  IconFileText,
  IconFolder,
  IconGitPullRequest,
  IconInfo,
  IconLink,
  IconListTree,
  IconMessage,
  IconPanelRightClose,
  IconPencil,
  IconTerminalSquare,
  IconTrash,
  IconUnlock,
} from "./icons";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";

type Props = {
  item: Item;
  /** Items of the visible workspaces, used to find the parent and children. */
  items: Item[];
  onSelect: (item: Item, tab?: Tab) => void;
  pipelines: Pipelines | undefined;
  /** Tab from the URL hash; unset falls back to the state-dependent default. */
  tab?: Tab;
  onTab: (tab: Tab) => void;
  /** Shows a board of the item's workspace, keeping the item open. */
  onBoard: (board: string) => void;
  onClose: () => void;
  onUnauthorized: () => void;
};

type Runner = ReturnType<typeof useRun>;

/** Shown while the item waits at a gate: discuss the brief, or approve it. */
function GateBar({ item, gate, runner: { busy, run }, onDiscuss }: { item: Item; gate: Gate; runner: Runner; onDiscuss?: () => void }) {
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-lg border border-primary/30 bg-primary/5 px-3 py-2.5 text-[15px]">
      <span className="text-primary">
        <IconInfo />
      </span>
      <span className="min-w-0 flex-1">Waiting for your decision. Approve moves it to {stageLabel(gate.approve_to)}.</span>
      {onDiscuss && (
        <Button disabled={busy} onClick={onDiscuss}>
          <IconMessage />
          Discuss brief
        </Button>
      )}
      <Button variant="primary" disabled={busy} onClick={() => run(gate.label, () => api.approve(item))}>
        <IconCheck />
        {gate.label}
      </Button>
    </div>
  );
}

/** Shown while the item is blocked: why, a button that clears it, and one that starts an agent session when none runs. */
function BlockedBar({ item, runner: { busy, run }, onStartAgent }: { item: Item; runner: Runner; onStartAgent?: () => void }) {
  const reason = item.agent_status.replace(/^blocked:?\s*/, "");
  return (
    <div className="flex flex-wrap items-start gap-x-3 gap-y-2 rounded-lg border border-blocked/30 bg-blocked/5 px-3 py-2.5 text-[15px]">
      <span className="flex h-7 items-center text-blocked">
        <IconInfo />
      </span>
      <span className="min-w-0 flex-1 py-1 leading-5 [overflow-wrap:anywhere]">
        <span className="font-medium text-blocked">Blocked</span>
        {reason && <> · {reason}</>}
      </span>
      {onStartAgent && (
        <Button disabled={busy} onClick={onStartAgent}>
          <IconBot />
          Start agent
        </Button>
      )}
      <Button disabled={busy} onClick={() => run("Clear blocked", () => api.clearBlocked(item))}>
        <IconUnlock />
        Clear
      </Button>
    </div>
  );
}

/** Stage, board (click to show it), issue and PR of the item, as the pills the card uses, then the agent status unless blocked, then `children`. */
function Properties({ item, pipelines, onBoard, children }: { item: Item; pipelines: Pipelines | undefined; onBoard: (board: string) => void; children?: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-1.5 *:max-w-full">
      <Badge>
        <span className={cn("size-2 shrink-0 rounded-full", STAGE_DOT[item.stage] ?? "bg-muted-foreground")} aria-hidden="true" />
        <span className="truncate capitalize">{stageTitle(item, pipelines)}</span>
      </Badge>
      <Badge asChild>
        <button type="button" className="cursor-pointer hover:underline" title={boardOfWorkspace(pipelines, item.workspace, item.board)?.pipeline?.repo ?? undefined} onClick={() => onBoard(shownBoard(item, pipelines))}>
          <IconBoard />
          <span className="truncate">{boardLabel(pipelines, item.workspace, item.board)}</span>
        </button>
      </Badge>
      <IssueBadge item={item} />
      <PrStatus item={item} />
      {!isBlocked(item) && <AgentStatus item={{ agent_status: item.agent_status, gate: null, approved: item.approved }} />}
      {children}
    </div>
  );
}

export function DetailPanel({ item, items, pipelines, onSelect, tab: routeTab, onTab, onBoard, onClose, onUnauthorized }: Props) {
  const key = `${item.workspace}/${item.dir}`;
  const {
    data: detail,
    error,
    reload,
  } = usePolling(() => api.detail(item.workspace, item.dir), 3000, key, onUnauthorized);
  const live = detail ?? item;
  const runner = useRun(() => void reload());
  /** The body the editor started from; set while the brief is being edited. */
  const [editing, setEditing] = useState<string>();
  /** False once the panel is gone; App keys it per item, so switching items unmounts it. */
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  /** Set by the follow-ups chip; the brief scrolls to its `## Follow-ups` once it is shown. */
  const [jump, setJump] = useState(false);
  const family = items.filter((i) => i.workspace === item.workspace);
  const children = family.filter((i) => i.parent === item.dir);
  const blockedChildren = children.filter(isBlocked).length;
  const waitsOn = openDependencies(live, items, pipelines).length;
  /** A blocked parent without a session is most likely held up by its children (the features board's watcher blocks it for them). */
  const tab = routeTab ?? (!live.session && isBlocked(live) && blockedChildren > 0 ? "children" : live.session || isBlocked(live) ? "terminal" : "brief");
  useEffect(() => {
    if (!jump || tab !== "brief" || editing !== undefined || !detail) return;
    // A tab's content mounts one render after the tab changes (Radix Presence), so wait a frame.
    const frame = requestAnimationFrame(() => {
      setJump(false);
      document.getElementById("follow-ups")?.scrollIntoView({ block: "start" });
    });
    return () => cancelAnimationFrame(frame);
  }, [jump, tab, editing, detail]);
  const kindIcon = KIND_ICONS[live.kind.trim().toLowerCase()];
  const path = itemTerminal(item.workspace, item.dir);
  const edit = () => {
    if (!detail) return;
    setEditing(stripFrontmatter(detail.content));
  };
  const remove = () =>
    runner.run(
      "Remove",
      async () => {
        const { worktree_left } = await api.remove(live);
        if (worktree_left) throw new Error(`Workitem removed, but the worktree stays: ${worktree_left}`);
      },
      `Remove workitem "${live.title}"?\nThe agent session is stopped, and the worktree and workitem folder are deleted. This cannot be recovered. Branch and PR stay.`,
    );
  const startAgent = () =>
    runner.run("Start session", async () => {
      await api.startSession(live);
      if (mounted.current) onTab("terminal");
    });
  const discuss = () =>
    runner.run("Discuss brief", async () => {
      await api.discussBrief(live);
      if (mounted.current) onTab("terminal");
    });

  return (
    <SidePanel aria-label="Workitem">
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-6 pt-5">
        <div className="grid gap-2.5">
          <div className="flex items-start gap-2">
            <h2 className="min-w-0 flex-1 text-[22px] leading-[1.3] font-semibold tracking-[-0.01em] [overflow-wrap:anywhere]">
              {kindIcon && (
                <span className="mr-2" aria-hidden="true">
                  {kindIcon}
                </span>
              )}
              {live.title}
            </h2>
            <Button variant="ghost" size="icon" className="-mt-0.5 -mr-2 shrink-0" aria-label="Close" onClick={onClose}>
              <IconPanelRightClose />
            </Button>
          </div>
          <Properties item={live} pipelines={pipelines} onBoard={onBoard}>
            <FollowUpsChip
              followUps={live.follow_ups}
              onClick={() => {
                setJump(true);
                onTab("brief");
              }}
            />
          </Properties>
          {isBlocked(live) && <BlockedBar item={live} runner={runner} onStartAgent={live.session ? undefined : startAgent} />}
          {live.gate && <GateBar item={live} gate={live.gate} runner={runner} onDiscuss={live.session ? undefined : discuss} />}
          <StageActions item={live} onDone={() => void reload()} />
          {runner.error && <ErrorText>{runner.error}</ErrorText>}
          {error && <ErrorText>{error}</ErrorText>}
        </div>
        <Tabs value={tab} activationMode="manual" onValueChange={(value) => onTab(value as Tab)} className="min-h-[420px] flex-1 gap-3">
          <div className="flex items-end gap-2 border-b border-border">
            <TabsList className="min-w-0 flex-1 flex-wrap border-b-0">
              <TabsTrigger value="brief">
                <IconFileText />
                Brief
              </TabsTrigger>
              <TabsTrigger value="info">
                <IconInfo />
                Info
              </TabsTrigger>
              <TabsTrigger value="files">
                <IconFolder />
                Files
              </TabsTrigger>
              {live.pr && (
                <TabsTrigger value="pr">
                  <IconGitPullRequest />
                  PR
                </TabsTrigger>
              )}
              <TabsTrigger value="dependencies">
                <IconLink />
                Depends on
                {waitsOn > 0 && <Badge variant="wait">{waitsOn}</Badge>}
              </TabsTrigger>
              {children.length > 0 && (
                <TabsTrigger value="children">
                  <IconListTree />
                  Children
                  {blockedChildren > 0 && <Badge variant="danger">{blockedChildren}</Badge>}
                </TabsTrigger>
              )}
              <TabsTrigger value="terminal">
                <IconTerminalSquare />
                Terminal
              </TabsTrigger>
            </TabsList>
            <div className="mb-1.5 flex shrink-0">
              {!live.session && (
                <Button
                  variant="ghost"
                  size="icon"
                  className="[&_svg:not([class*='size-'])]:size-4"
                  aria-label="Start agent session"
                  title="Start agent session"
                  disabled={runner.busy}
                  onClick={startAgent}
                >
                  <IconBot />
                </Button>
              )}
              <Button
                variant="ghost"
                size="icon"
                className="text-destructive hover:text-destructive [&_svg:not([class*='size-'])]:size-4"
                aria-label="Remove"
                title="Remove workitem"
                disabled={runner.busy}
                onClick={remove}
              >
                <IconTrash />
              </Button>
            </div>
          </div>
          <TabsContent value="brief" className="flex min-h-0 flex-col">
            {editing !== undefined ? (
              <BriefEditor
                item={live}
                base={editing}
                onClose={(saved) => {
                  setEditing(undefined);
                  if (saved) void reload();
                }}
              />
            ) : (
              <div className={cn("brief", scroll, prose)}>
                <ActionBar>
                  {!live.session && (
                    <Button disabled={runner.busy} onClick={discuss}>
                      <IconMessage />
                      Discuss brief
                    </Button>
                  )}
                  <Button disabled={runner.busy || !detail} onClick={edit}>
                    <IconPencil />
                    Edit
                  </Button>
                </ActionBar>
                {detail ? <BriefView item={live} body={stripFrontmatter(detail.content)} onSaved={() => void reload()} /> : <Muted>Loading…</Muted>}
              </div>
            )}
          </TabsContent>
          <TabsContent value="info" className={scroll}>
            {detail ? <Info item={live} content={detail.content} onSaved={() => void reload()} /> : <Muted>Loading…</Muted>}
          </TabsContent>
          <TabsContent value="files" className="flex min-h-0 flex-col">
            <Files item={live} onUnauthorized={onUnauthorized} />
          </TabsContent>
          {live.pr && (
            <TabsContent value="pr" className={cn("brief", scroll, prose)}>
              <PrActions item={live} />
              {live.review?.state === "open" && live.review.failed.length > 0 && (
                <div className="mb-3">
                  <strong>Failing checks</strong>
                  <ul className="mt-1 text-destructive">
                    {live.review.failed.map((name) => (
                      <li key={name}>
                        <a href={`${live.pr}/checks`} target="_blank" rel="noreferrer">
                          {name} ↗
                        </a>
                      </li>
                    ))}
                  </ul>
                </div>
              )}
              <PrConversation item={live} onUnauthorized={onUnauthorized} />
            </TabsContent>
          )}
          <TabsContent value="dependencies" className={scroll}>
            <Dependencies item={live} items={items} pipelines={pipelines} onSelect={onSelect} onSaved={() => void reload()} />
          </TabsContent>
          {children.length > 0 && (
            <TabsContent value="children" className={scroll}>
              <Children items={children} onSelect={onSelect} onDone={() => void reload()} />
            </TabsContent>
          )}
          <TabsContent value="terminal" className="flex min-h-0 flex-col pb-4">
            <AgentActions item={live} onDone={() => void reload()} />
            {live.session && <Terminal path={path} />}
          </TabsContent>
        </Tabs>
      </div>
    </SidePanel>
  );
}
