import { FormEvent, ReactNode, useEffect, useState } from "react";
import { api, itemTerminal } from "./api";
import { AgentStatus } from "./AgentStatus";
import { boardLabel, boardOfWorkspace, isBlocked, KIND_ICONS, openDependencies, repoName, stageLabel, stageTitle, stripFrontmatter, ticket } from "./boardState";
import { Gate, Item, Pipelines } from "./types";
import { Actions } from "./Actions";
import { AgentActions } from "./AgentActions";
import { BriefEditor } from "./BriefEditor";
import { BriefView } from "./BriefView";
import { Children } from "./Children";
import { Dependencies } from "./Dependencies";
import { Dialog, DialogForm, Field } from "./Dialog";
import { PrActions } from "./PrActions";
import { PrConversation } from "./PrConversation";
import { PrStatus } from "./PrStatus";
import { Files } from "./Files";
import { FollowUpsChip } from "./FollowUpsChip";
import { Info } from "./Info";
import { IssueStatusChip } from "./IssueStatusChip";
import { ErrorText, Muted, prose, scroll, SidePanel } from "./parts";
import { ReviewStrip } from "./ReviewStrip";
import { STAGE_DOT } from "./tone";
import { Tab } from "./url";
import { Terminal } from "./Terminal";
import { usePolling } from "./usePolling";
import { useRun } from "./useRun";
import {
  IconCheck,
  IconExternal,
  IconFileText,
  IconFolder,
  IconGitBranch,
  IconGitPullRequest,
  IconInfo,
  IconLink,
  IconListTree,
  IconMessage,
  IconPencil,
  IconSliders,
  IconTerminalSquare,
  IconUndo,
  IconX,
} from "./icons";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/input";
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
  onClose: () => void;
  onUnauthorized: () => void;
};

type Runner = ReturnType<typeof useRun>;

/** Shown while the item waits at a gate: approve it, or send it back with a comment. */
function GateBar({ item, gate, runner: { busy, run } }: { item: Item; gate: Gate; runner: Runner }) {
  const [sending, setSending] = useState(false);
  const [comment, setComment] = useState("");
  const sendBack = (event: FormEvent) => {
    event.preventDefault();
    setSending(false);
    run("Send back", () => api.reject(item, comment));
  };
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-lg border border-primary/30 bg-primary/5 px-3 py-2.5 text-[13px]">
      <span className="text-primary">
        <IconInfo />
      </span>
      <span className="min-w-0 flex-1">Waiting for your decision. Approve moves it to {stageLabel(gate.approve_to)}.</span>
      <Button disabled={busy} onClick={() => setSending(true)}>
        <IconUndo />
        Send back
      </Button>
      <Button variant="primary" disabled={busy} onClick={() => run(gate.label, () => api.approve(item))}>
        <IconCheck />
        {gate.label}
      </Button>
      {sending && (
        <Dialog title={`Send back to ${stageLabel(gate.reject_to)}`} onClose={() => setSending(false)}>
          <DialogForm onSubmit={sendBack}>
            <Field label="Comment">
              <Textarea value={comment} onChange={(e) => setComment(e.target.value)} placeholder="Why is this rejected?" rows={4} required autoFocus />
            </Field>
            <Button type="submit" variant="primary" disabled={!comment.trim()}>
              Send back
            </Button>
          </DialogForm>
        </Dialog>
      )}
    </div>
  );
}

function Property({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="flex min-w-0 items-center gap-1.5 [overflow-wrap:anywhere]">{children}</dd>
    </>
  );
}

const linkButton = "cursor-pointer truncate text-left text-primary hover:underline";

/** Stage, type, parent, board and repo, branch, PR and issue of the item. */
function Properties({ item, parent, pipelines, onSelect }: { item: Item; parent: Item | undefined; pipelines: Pipelines | undefined; onSelect: (item: Item) => void }) {
  const kindIcon = KIND_ICONS[item.kind.trim().toLowerCase()];
  const repo = boardOfWorkspace(pipelines, item.workspace, item.board)?.pipeline?.repo;
  const issue = ticket(item.issue);
  return (
    <dl className="grid grid-cols-[96px_minmax(0,1fr)] items-center gap-x-3 gap-y-2 text-[13px]">
      <Property label="Stage">
        <span className={cn("size-2 shrink-0 rounded-full", STAGE_DOT[item.stage] ?? "bg-muted-foreground")} aria-hidden="true" />
        <span className="font-medium capitalize">{stageTitle(item, pipelines)}</span>
      </Property>
      <Property label="Type">
        {kindIcon && <span aria-hidden="true">{kindIcon}</span>}
        {item.kind}
      </Property>
      {item.parent && (
        <Property label="Parent">
          {parent ? (
            <button className={linkButton} onClick={() => onSelect(parent)}>
              {parent.title}
            </button>
          ) : (
            <span className="font-mono text-[12px]">{item.parent}</span>
          )}
        </Property>
      )}
      <Property label="Board">
        <span>{boardLabel(pipelines, item.workspace, item.board)}</span>
        {repo && (
          <span className="truncate font-mono text-[11.5px] text-muted-foreground" title={repo}>
            {repoName(repo)}
          </span>
        )}
      </Property>
      {item.branch && (
        <Property label="Branch">
          <span className="text-muted-foreground">
            <IconGitBranch />
          </span>
          <span className="font-mono text-[12px]">{item.branch}</span>
        </Property>
      )}
      {item.pr && (
        <Property label="PR">
          <PrStatus item={item} className="text-[13px]" />
        </Property>
      )}
      {issue && (
        <Property label="Issue">
          {issue.url ? (
            <a className="text-primary no-underline hover:underline" href={issue.url} target="_blank" rel="noreferrer">
              {issue.key} ↗
            </a>
          ) : (
            <span>{issue.key}</span>
          )}
          <IssueStatusChip status={item.issue_status} category={item.issue_status_category} />
        </Property>
      )}
    </dl>
  );
}

export function DetailPanel({ item, items, pipelines, onSelect, tab: routeTab, onTab, onClose, onUnauthorized }: Props) {
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
  useEffect(() => setEditing(undefined), [key]);
  /** Set by the follow-ups chip; the brief scrolls to its `## Follow-ups` once it is shown. */
  const [jump, setJump] = useState(false);
  const family = items.filter((i) => i.workspace === item.workspace);
  const parent = family.find((i) => i.dir === live.parent);
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
  const path = itemTerminal(item.workspace, item.dir);
  const popOut = `/?terminal=${encodeURIComponent(path)}`;
  const edit = () => {
    if (!detail) return;
    setEditing(stripFrontmatter(detail.content));
    onTab("brief");
  };
  const discuss = () =>
    runner.run("Discuss brief", async () => {
      await api.discussBrief(live);
      onTab("terminal");
    });

  return (
    <SidePanel aria-label="Workitem">
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border pr-4 pl-5">
        <span className="min-w-0 flex-1 truncate font-mono text-[11.5px] text-muted-foreground" title={live.dir}>
          {live.dir}
        </span>
        {!live.session && (
          <Button disabled={runner.busy} onClick={discuss}>
            <IconMessage />
            Discuss brief
          </Button>
        )}
        <Button disabled={runner.busy || !detail || editing !== undefined} onClick={edit}>
          <IconPencil />
          Edit
        </Button>
        <Button variant="ghost" size="icon" aria-label="Close" onClick={onClose}>
          <IconX />
        </Button>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-6 pt-5">
        <div className="grid gap-2.5">
          <h2 className="text-[20px] leading-[1.3] font-semibold tracking-[-0.01em] [overflow-wrap:anywhere]">{live.title}</h2>
          <div className="flex flex-wrap items-center gap-1.5 empty:hidden">
            <AgentStatus item={{ agent_status: live.agent_status, gate: null }} className="basis-full" />
            <FollowUpsChip
              followUps={live.follow_ups}
              onClick={() => {
                setJump(true);
                onTab("brief");
              }}
            />
            {!!live.unpushed && <Badge title="Commits in the worktree that are not on any remote">{live.unpushed} unpushed</Badge>}
            <ReviewStrip review={live.review} />
          </div>
          {live.gate && <GateBar item={live} gate={live.gate} runner={runner} />}
          {runner.error && <ErrorText>{runner.error}</ErrorText>}
          {error && <ErrorText>{error}</ErrorText>}
        </div>
        <Properties item={live} parent={parent} pipelines={pipelines} onSelect={onSelect} />
        <Tabs value={tab} onValueChange={(value) => onTab(value as Tab)} className="min-h-[420px] flex-1 gap-3">
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
              <TabsTrigger value="actions">
                <IconSliders />
                Actions
              </TabsTrigger>
            </TabsList>
            {live.session && (
              <a className="mb-2 inline-flex shrink-0 items-center gap-1.5 text-[13px] text-primary no-underline hover:underline" href={popOut} target="_blank" rel="noreferrer">
                <IconExternal />
                Pop out
              </a>
            )}
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
              <PrActions item={live} onDone={() => void reload()} />
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
          <TabsContent value="actions" className={scroll}>
            <Actions item={live} onDone={() => void reload()} />
          </TabsContent>
        </Tabs>
      </div>
    </SidePanel>
  );
}
