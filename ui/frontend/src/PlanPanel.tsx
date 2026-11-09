import { useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api, planTerminal } from "./api";
import { IconExternal, IconFileText, IconTerminalSquare, IconX } from "./icons";
import { planLabel, planStatus } from "./planState";
import { Terminal } from "./Terminal";
import { Plan } from "./types";
import { usePolling } from "./usePolling";

type Props = {
  plan: Plan;
  /** Opens the workitem made from this plan. */
  onWorkitem: (workspace: string, dir: string) => void;
  onClose: () => void;
  onChanged: () => void;
  onUnauthorized: () => void;
};

/** Right panel for one planning session: its terminal (or Resume) and the saved plan. */
export function PlanPanel({ plan, onWorkitem, onClose, onChanged, onUnauthorized }: Props) {
  const key = `${plan.workspace}/${plan.id}`;
  const { data: detail, error: loadError, reload } = usePolling(() => api.plan(plan.workspace, plan.id), 3000, key, onUnauthorized);
  const live = detail ?? plan;
  const status = planStatus(live);
  const [tab, setTab] = useState<"terminal" | "plan">();
  const shown = tab === "plan" && !detail?.text ? "terminal" : tab ?? (live.live || !detail?.text ? "terminal" : "plan");
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
    <aside className="detail">
      <header>
        <div>
          <h2>{live.topic || live.id}</h2>
          <div className="meta">
            <span className="badge">{live.workspace}</span>
            <span className={`chip plan-status ${status}`}>{planLabel(status)}</span>
            {live.workitems.map((dir) => (
              <button key={dir} className="link" onClick={() => onWorkitem(live.workspace, dir)}>
                → {dir}
              </button>
            ))}
          </div>
          {live.summary && <p className="plan-panel-summary">{live.summary}</p>}
        </div>
        <button className="link" onClick={onClose}>
          <IconX />
          Close
        </button>
      </header>
      <nav className="tabs">
        <button className={shown === "terminal" ? "active" : ""} onClick={() => setTab("terminal")}>
          <IconTerminalSquare />
          Terminal
        </button>
        {detail?.text && (
          <button className={shown === "plan" ? "active" : ""} onClick={() => setTab("plan")}>
            <IconFileText />
            Plan
          </button>
        )}
        {live.live && (
          <a href={`/?terminal=${encodeURIComponent(path)}`} target="_blank" rel="noreferrer">
            <IconExternal />
            Pop out
          </a>
        )}
        <button className="plan-delete" onClick={() => void remove()} disabled={busy}>
          Delete
        </button>
      </nav>
      {(error ?? loadError) && <p className="error">{error ?? loadError}</p>}
      {shown === "terminal" &&
        (live.live ? (
          <Terminal key={path} path={path} />
        ) : (
          <div className="brief">
            <p>The session has ended. Resume continues the same Claude conversation in the repo.</p>
            <button onClick={() => void resume()} disabled={busy}>
              Resume
            </button>
            {detail?.prompt && (
              <>
                <h3>Started with</h3>
                <Markdown remarkPlugins={[remarkGfm]}>{detail.prompt}</Markdown>
              </>
            )}
          </div>
        ))}
      {shown === "plan" && detail?.text && (
        <div className="brief">
          <Markdown remarkPlugins={[remarkGfm]}>{detail.text}</Markdown>
        </div>
      )}
    </aside>
  );
}
