import { useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api, planTerminal } from "./api";
import { Children } from "./Children";
import { IconFileText, IconListTree, IconTerminalSquare, IconTrash, IconX } from "./icons";
import { ErrorText, Muted, prose, scroll, SidePanel } from "./parts";
import { planLabel, planStatus } from "./planState";
import { Terminal } from "./Terminal";
import { Item, Plan } from "./types";
import { Tab as ItemTab } from "./url";
import { usePolling } from "./usePolling";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";

type Tab = "terminal" | "plan" | "workitems";

type Props = {
  plan: Plan;
  /** The workitems made from this plan that still exist. */
  workitems: Item[];
  onSelect: (item: Item, tab?: ItemTab) => void;
  onItemsChanged: () => void;
  onClose: () => void;
  onChanged: () => void;
  onUnauthorized: () => void;
};

/** Right panel for one planning session: its terminal (or Resume), the saved plan and the workitems made from it. */
export function PlanPanel({ plan, workitems, onSelect, onItemsChanged, onClose, onChanged, onUnauthorized }: Props) {
  const key = `${plan.workspace}/${plan.id}`;
  const { data: detail, error: loadError, reload } = usePolling(() => api.plan(plan.workspace, plan.id), 3000, key, onUnauthorized);
  const live = detail ?? plan;
  const status = planStatus(live);
  const [tab, setTab] = useState<Tab>();
  const gone = (tab === "plan" && !detail?.text) || (tab === "workitems" && !workitems.length);
  const shown = gone ? "terminal" : tab ?? (live.live || !detail?.text ? "terminal" : "plan");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const path = planTerminal(live);

  /** Runs a button's request; returns whether it worked. */
  const act = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(undefined);
    try {
      await action();
      onChanged();
      return true;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const resume = async () => {
    if (await act(() => api.resumePlan(live))) void reload();
  };

  const remove = async () => {
    const what = live.live ? "Stop the session and delete" : "Delete";
    if (!confirm(`${what} the plan "${live.topic}"? Its Claude conversation can no longer be resumed from the UI.`)) return;
    if (await act(() => api.deletePlan(live))) onClose();
  };

  return (
    <SidePanel aria-label="Plan">
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-6 pt-5">
        <div className="grid gap-2.5">
          <div className="flex items-start gap-2">
            <h2 className="min-w-0 flex-1 text-[20px] leading-[1.3] font-semibold tracking-[-0.01em] [overflow-wrap:anywhere]">
              <span className="mr-2 inline-flex align-[-0.1em] text-muted-foreground [&>svg]:size-[18px]">
                <IconFileText />
              </span>
              {live.topic || live.id}
            </h2>
            <Button variant="ghost" size="icon" className="-mt-0.5 -mr-2 shrink-0" aria-label="Close" onClick={onClose}>
              <IconX />
            </Button>
          </div>
          {live.summary && <Muted>{live.summary}</Muted>}
          <div className="flex flex-wrap items-center gap-1.5">
            <Badge variant={status === "working" ? "good" : status === "waiting" ? "wait" : "outline"}>{planLabel(status)}</Badge>
          </div>
          {(error ?? loadError) && <ErrorText>{error ?? loadError}</ErrorText>}
        </div>
        <Tabs value={shown} activationMode="manual" onValueChange={(value) => setTab(value as Tab)} className="min-h-[420px] flex-1 gap-3">
          <div className="flex items-end gap-2 border-b border-border">
            <TabsList className="min-w-0 flex-1 border-b-0">
              <TabsTrigger value="terminal">
                <IconTerminalSquare />
                Terminal
              </TabsTrigger>
              {detail?.text && (
                <TabsTrigger value="plan">
                  <IconFileText />
                  Plan
                </TabsTrigger>
              )}
              {workitems.length > 0 && (
                <TabsTrigger value="workitems">
                  <IconListTree />
                  Workitems
                  <Badge>{workitems.length}</Badge>
                </TabsTrigger>
              )}
            </TabsList>
            <Button variant="ghost" className="mb-1.5 shrink-0 text-destructive hover:text-destructive" onClick={() => void remove()} disabled={busy}>
              <IconTrash />
              Delete
            </Button>
          </div>
          <TabsContent value="terminal" className="flex min-h-0 flex-col pb-4">
            {live.live ? (
              <Terminal key={path} path={path} />
            ) : (
              <div className={cn("brief", scroll, prose)}>
                <p>The session has ended. Resume continues the same Claude conversation in the repo.</p>
                <Button variant="primary" className="mb-3" onClick={() => void resume()} disabled={busy}>
                  Resume
                </Button>
                {detail?.prompt && (
                  <>
                    <h3>Started with</h3>
                    <Markdown remarkPlugins={[remarkGfm]}>{detail.prompt}</Markdown>
                  </>
                )}
              </div>
            )}
          </TabsContent>
          {detail?.text && (
            <TabsContent value="plan" className={cn("brief", scroll, prose)}>
              <Markdown remarkPlugins={[remarkGfm]}>{detail.text}</Markdown>
            </TabsContent>
          )}
          {workitems.length > 0 && (
            <TabsContent value="workitems" className={scroll}>
              <Children items={workitems} onSelect={onSelect} onDone={onItemsChanged} />
            </TabsContent>
          )}
        </Tabs>
      </div>
    </SidePanel>
  );
}
