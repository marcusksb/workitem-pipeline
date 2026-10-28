import { useEffect, useState } from "react";

import { api } from "./api";
import { Dialog } from "./Dialog";
import { Source, StageSource as Sources } from "./types";

type Props = { label: string; stage: string; board: string; projects: string[]; onClose: () => void };

function File({ source }: { source: Source }) {
  const where = [source.origin, source.note].filter(Boolean).join(", ");
  return (
    <div className="source">
      <p className="source-path">
        <strong>{source.name}</strong>
        {source.path && <code title={source.path}>{source.path}</code>}
        {where && <span className="badge">{where}</span>}
      </p>
      {source.error ? <p className="error">{source.error}</p> : <pre>{source.content}</pre>}
    </div>
  );
}

/** What a stage runs, read-only: its watch scripts and its handler skill (or inline prompt). */
export function StageSource({ label, stage, board, projects, onClose }: Props) {
  const [project, setProject] = useState(projects[0]);
  const [sources, setSources] = useState<Sources>();
  const [error, setError] = useState<string>();
  const [tab, setTab] = useState<"script" | "prose">();

  useEffect(() => {
    setSources(undefined);
    setError(undefined);
    api.stageSource(project, board, stage).then(setSources, (e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, [project, board, stage]);

  const hasScript = !!sources?.scripts.length;
  const hasProse = !!sources?.prose;
  const shown = tab === "script" && hasScript ? "script" : tab === "prose" && hasProse ? "prose" : hasProse ? "prose" : "script";
  return (
    <Dialog title={`${label}: what runs`} onClose={onClose}>
      {projects.length > 1 && (
        <select value={project} onChange={(e) => setProject(e.target.value)} aria-label="Project">
          {projects.map((name) => <option key={name}>{name}</option>)}
        </select>
      )}
      <div className="tabs" role="tablist">
        <button role="tab" aria-selected={shown === "prose"} className={shown === "prose" ? "active" : ""} disabled={!hasProse} onClick={() => setTab("prose")}>
          Skill / prompt
        </button>
        <button role="tab" aria-selected={shown === "script"} className={shown === "script" ? "active" : ""} disabled={!hasScript} onClick={() => setTab("script")}>
          Script
        </button>
      </div>
      {error && <p className="error">{error}</p>}
      {!sources && !error && <p className="empty">Loading…</p>}
      {sources && shown === "prose" && sources.prose && <File source={sources.prose} />}
      {sources && shown === "script" && sources.scripts.map((script, i) => <File key={i} source={script} />)}
      {sources && !hasProse && !hasScript && <p className="empty">This stage runs nothing.</p>}
    </Dialog>
  );
}
