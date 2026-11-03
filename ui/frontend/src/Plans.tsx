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
  /** Projects in view; new plans go to the first unless the dialog picks another. */
  projects: string[];
  showProject: boolean;
  selected: Plan | null;
  onSelect: (plan: Plan) => void;
  onStarted: (project: string, id: string) => void;
};

/** The planning sessions of the projects in view, above the board (decision 36). */
export function Plans({ plans, sessions, projects, showProject, selected, onSelect, onStarted }: Props) {
  const [collapsed, setCollapsed] = useState(loadCollapsed);
  const [showDone, setShowDone] = useState(false);
  const [creating, setCreating] = useState(false);
  const { open, done } = groupPlans(plans, projects);
  const shownSessions = sessions.filter((session) => projects.includes(session.project));
  if (!projects.length) return null;

  const toggle = () => {
    setCollapsed(!collapsed);
    saveCollapsed(!collapsed);
  };
  const card = (plan: Plan) => {
    const status = planStatus(plan);
    const isSelected = selected?.project === plan.project && selected.id === plan.id;
    return (
      <button key={`${plan.project}/${plan.id}`} className={`plan-card ${status}${isSelected ? " selected" : ""}`} onClick={() => onSelect(plan)}>
        <span className="plan-topic">
          <span className={`plan-dot ${status}`} title={planLabel(status)} />
          {plan.topic || plan.id}
        </span>
        <span className="plan-summary">{plan.summary || (status === "ended" ? "No summary" : "Starting…")}</span>
        {(showProject || plan.workitem) && (
          <span className="plan-meta">
            {showProject && <span className="badge">{plan.project}</span>}
            {plan.workitem && <span className="badge">→ {plan.workitem}</span>}
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
            key={`${session.project}/${session.name}`}
            className="plans-session"
            href={`/?terminal=${encodeURIComponent(agentTerminal(session.project, session.name))}`}
            target="_blank"
            rel="noreferrer"
            title="A workitem is being created; a blocked one can only be answered here"
          >
            {session.project} · {session.name} ↗
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
          projects={projects}
          onClose={() => setCreating(false)}
          onStarted={(project, id) => {
            setCreating(false);
            onStarted(project, id);
          }}
        />
      )}
    </section>
  );
}

function NewPlan({ projects, onClose, onStarted }: { projects: string[]; onClose: () => void; onStarted: (project: string, id: string) => void }) {
  const [project, setProject] = useState(projects[0]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    try {
      const { id } = await api.createPlan(project, input);
      onStarted(project, id);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };

  return (
    <Dialog title="New planning session" onClose={onClose}>
      <form className="dialog-form" onSubmit={submit}>
        {projects.length > 1 && (
          <label>
            Project
            <select value={project} onChange={(e) => setProject(e.target.value)}>
              {projects.map((name) => (
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
