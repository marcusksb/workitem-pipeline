import { FormEvent, useState } from "react";
import { agentTerminal, api } from "./api";
import { Dialog } from "./Dialog";
import { groupPlans, planCounts, planLabel, planStatus } from "./planState";
import { AgentSession, Plan } from "./types";

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
    // Not remembered; the bar still works.
  }
}

type Props = {
  plans: Plan[];
  /** `wi-new-*` sessions (workitem create), shown as links at the end. */
  sessions: AgentSession[];
  /** Workspaces in view; new plans go to the first unless the dialog picks another. */
  workspaces: string[];
  showWorkspace: boolean;
  selected: Plan | null;
  onSelect: (plan: Plan) => void;
  onStarted: (workspace: string, id: string) => void;
};

/** The planning sessions of the workspaces in view, above the board (decision 36). */
export function Plans({ plans, sessions, workspaces, showWorkspace, selected, onSelect, onStarted }: Props) {
  const [collapsed, setCollapsed] = useState(loadCollapsed);
  const [showDone, setShowDone] = useState(false);
  const [creating, setCreating] = useState(false);
  const { open, done } = groupPlans(plans, workspaces);
  const shownSessions = sessions.filter((session) => workspaces.includes(session.workspace));
  if (!workspaces.length) return null;

  const toggle = () => {
    setCollapsed(!collapsed);
    saveCollapsed(!collapsed);
  };
  const card = (plan: Plan) => {
    const status = planStatus(plan);
    const isSelected = selected?.workspace === plan.workspace && selected.id === plan.id;
    return (
      <button key={`${plan.workspace}/${plan.id}`} className={`plan-card ${status}${isSelected ? " selected" : ""}`} onClick={() => onSelect(plan)}>
        <span className="plan-topic">
          <span className={`plan-dot ${status}`} title={planLabel(status)} />
          {plan.topic || plan.id}
        </span>
        <span className="plan-summary">{plan.summary || (status === "ended" ? "No summary" : "Starting…")}</span>
        {(showWorkspace || plan.workitems.length > 0) && (
          <span className="plan-meta">
            {showWorkspace && <span className="badge">{plan.workspace}</span>}
            {plan.workitems.map((dir) => (
              <span key={dir} className="badge">→ {dir}</span>
            ))}
          </span>
        )}
      </button>
    );
  };

  return (
    <section className={`plans${collapsed ? " collapsed" : ""}`} aria-label="Planning sessions">
      <div className="plans-head">
        <button className="link plans-toggle" onClick={toggle} aria-expanded={!collapsed}>
          {collapsed ? "▸" : "▾"} Plans · {planCounts(open)}
        </button>
        <button className="link" onClick={() => setCreating(true)}>
          + Plan
        </button>
        {shownSessions.map((session) => (
          <a
            key={`${session.workspace}/${session.name}`}
            className="plans-session"
            href={`/?terminal=${encodeURIComponent(agentTerminal(session.workspace, session.name))}`}
            target="_blank"
            rel="noreferrer"
            title="A workitem is being created; a blocked one can only be answered here"
          >
            {session.workspace} · {session.name} ↗
          </a>
        ))}
      </div>
      {!collapsed && (open.length > 0 || done.length > 0) && (
        <div className="plans-cards">
          {open.map(card)}
          {done.length > 0 && (
            <button className="link plans-done" onClick={() => setShowDone(!showDone)} aria-expanded={showDone}>
              {showDone ? "Hide" : "Show"} {done.length} done
            </button>
          )}
          {showDone && done.map(card)}
        </div>
      )}
      {creating && (
        <NewPlan
          workspaces={workspaces}
          onClose={() => setCreating(false)}
          onStarted={(workspace, id) => {
            setCreating(false);
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
      <form className="dialog-form" onSubmit={submit}>
        {workspaces.length > 1 && (
          <label>
            Workspace
            <select value={workspace} onChange={(e) => setWorkspace(e.target.value)}>
              {workspaces.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
          </label>
        )}
        <label>
          What do you want to plan?
          <textarea value={input} onChange={(e) => setInput(e.target.value)} placeholder="The first line becomes the topic" rows={5} required autoFocus />
        </label>
        <p className="hint">Runs Claude in the repo root. It can read everything and create workitems, but cannot change code.</p>
        <button type="submit" disabled={busy}>
          Start
        </button>
        {error && <span className="error">{error}</span>}
      </form>
    </Dialog>
  );
}
