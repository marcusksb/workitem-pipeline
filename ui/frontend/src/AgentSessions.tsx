import { agentTerminal } from "./api";
import { AgentSession } from "./types";

/** Sessions without a workitem (create). A blocked one can only be answered by attaching. */
export function AgentSessions({ sessions }: { sessions: AgentSession[] }) {
  if (!sessions.length) return null;
  return (
    <div className="sessions">
      <span>Agent sessions:</span>
      {sessions.map((session) => (
        <a
          key={`${session.project}/${session.name}`}
          href={`/?terminal=${encodeURIComponent(agentTerminal(session.project, session.name))}`}
          target="_blank"
          rel="noreferrer"
        >
          {session.project} · {session.name} ↗
        </a>
      ))}
    </div>
  );
}
