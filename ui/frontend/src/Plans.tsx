import { FormEvent, useState } from "react";
import { api } from "./api";
import { Button } from "@/components/ui/button";
import { NativeSelect, Textarea } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { Dialog, DialogForm, Field } from "./Dialog";
import { IconChevronDown, IconPlus } from "./icons";
import { ErrorText, Muted } from "./parts";
import { navExtra, navHeading, navItem, navLabel } from "./Sidebar";
import { groupPlans, planCounts, planLabel, planStatus } from "./planState";
import { Plan } from "./types";

const COLLAPSED_KEY = "workload.plans.collapsed";

function loadCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

function saveCollapsed(collapsed: boolean) {
  try {
    localStorage.setItem(COLLAPSED_KEY, collapsed ? "1" : "0");
  } catch {
    // Not remembered; the list still works.
  }
}

type Props = {
  plans: Plan[];
  /** Workspaces in view; new plans go to the first unless the dialog picks another. */
  workspaces: string[];
  showWorkspace: boolean;
  selected: Plan | null;
  onSelect: (plan: Plan) => void;
  onStarted: (workspace: string, id: string) => void;
  /** Whether the New plan dialog is open; App owns it so the palette can open it too. */
  creating: boolean;
  onCreating: (creating: boolean) => void;
};

/** The planning sessions of the workspaces in view, in the sidebar (decision 36). */
export function Plans({ plans, workspaces, showWorkspace, selected, onSelect, onStarted, creating, onCreating }: Props) {
  const [collapsed, setCollapsed] = useState(loadCollapsed);
  const [showDone, setShowDone] = useState(false);
  const { open, done } = groupPlans(plans, workspaces);
  if (!workspaces.length) return null;

  const toggle = () => {
    setCollapsed(!collapsed);
    saveCollapsed(!collapsed);
  };
  const card = (plan: Plan) => {
    const status = planStatus(plan);
    const isSelected = selected?.workspace === plan.workspace && selected.id === plan.id;
    const summary = plan.summary || (status === "ended" ? "No summary" : "Starting…");
    return (
      <Button
        key={`${plan.workspace}/${plan.id}`}
        variant="ghost"
        className={cn(navItem, "plan-card h-auto min-h-9 items-start gap-2 py-2 whitespace-normal group-data-[collapsed=true]/sidebar:items-center", status, isSelected && "selected bg-foreground/[0.07] font-medium", status === "ended" && "text-muted-foreground")}
        title={`${plan.topic || plan.id}\n${planLabel(status)}: ${summary}`}
        aria-current={isSelected || undefined}
        onClick={() => onSelect(plan)}
      >
        <span className={cn("mt-[7px] size-2 shrink-0 rounded-full group-data-[collapsed=true]/sidebar:mt-0", status === "working" ? "bg-good" : status === "waiting" ? "bg-working" : "bg-muted-foreground")} aria-hidden="true" />
        <span className={cn("line-clamp-2 flex-1 text-left leading-snug", navLabel)}>{plan.topic || plan.id}</span>
        {showWorkspace && <span className={cn("text-[13.5px] text-muted-foreground", navExtra)}>{plan.workspace}</span>}
      </Button>
    );
  };

  return (
    <section className="plans flex flex-col gap-px group-data-[collapsed=true]/sidebar:border-t group-data-[collapsed=true]/sidebar:border-border group-data-[collapsed=true]/sidebar:pt-3.5" aria-label="Planning sessions">
      <div className="flex items-center group-data-[collapsed=true]/sidebar:justify-center">
        <Button variant="ghost" size="sm" className={cn(navHeading, navExtra, "plans-toggle h-auto flex-1 justify-start hover:bg-foreground/5")} onClick={toggle} aria-expanded={!collapsed}>
          Plans · {planCounts(open)}
          <span className={collapsed ? "-rotate-90" : undefined}>
            <IconChevronDown />
          </span>
        </Button>
        <Button variant="ghost" size="icon" className="size-[22px]" aria-label="New plan" title="Start a planning session" onClick={() => onCreating(true)}>
          <IconPlus />
        </Button>
      </div>
      {!collapsed && (
        <>
          {open.map(card)}
          {done.length > 0 && (
            <Button variant="ghost" size="sm" className={cn(navItem, navExtra, "h-6 text-xs text-muted-foreground")} onClick={() => setShowDone(!showDone)} aria-expanded={showDone}>
              {showDone ? "Hide" : "Show"} {done.length} done
            </Button>
          )}
          {showDone && done.map(card)}
        </>
      )}
      {creating && (
        <NewPlan
          workspaces={workspaces}
          onClose={() => onCreating(false)}
          onStarted={(workspace, id) => {
            onCreating(false);
            onStarted(workspace, id);
          }}
        />
      )}
    </section>
  );
}

function NewPlan({ workspaces, onClose, onStarted }: { workspaces: string[]; onClose: () => void; onStarted: (workspace: string, id: string) => void }) {
  const [workspace, setWorkspace] = useState(workspaces[0]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    try {
      const { id } = await api.createPlan(workspace, input);
      onStarted(workspace, id);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };

  return (
    <Dialog title="New planning session" onClose={onClose}>
      <DialogForm onSubmit={submit}>
        {workspaces.length > 1 && (
          <Field label="Workspace">
            <NativeSelect value={workspace} onChange={(e) => setWorkspace(e.target.value)}>
              {workspaces.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </NativeSelect>
          </Field>
        )}
        <Field label="What do you want to plan?">
          <Textarea value={input} onChange={(e) => setInput(e.target.value)} placeholder="The first line becomes the topic" rows={5} required autoFocus />
        </Field>
        <Muted>Runs the workspace's agent in the repo root. It can read everything and create workitems, but cannot change code.</Muted>
        <Button type="submit" variant="primary" disabled={busy}>
          Start
        </Button>
        {error && <ErrorText>{error}</ErrorText>}
      </DialogForm>
    </Dialog>
  );
}
