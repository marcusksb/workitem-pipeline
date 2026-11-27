import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { agentTerminal, api } from "./api";
import { BoardProblems } from "./boardState";
import { IconAlert, IconBot, IconExternal } from "./icons";
import { ErrorText } from "./parts";
import { AgentSession } from "./types";

type Props = {
  /** One workspace's problems with the board on screen. */
  found: BoardProblems;
  board: string;
  boardLabel: string;
  /** Sessions of every workspace; a live `fix` one of this workspace replaces the button. */
  sessions: AgentSession[];
  /** More than one workspace is on screen: name this one. */
  showWorkspace: boolean;
};

/** The problems of a board above its columns, with a button that starts an agent to fix them (decision 85). */
export function BoardProblemsPanel({ found, board, boardLabel, sessions, showWorkspace }: Props) {
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string>();
  const { workspace, problems } = found;
  const fixing = sessions.find((s) => s.workspace === workspace && s.purpose === "fix" && !s.dead);
  const fix = async () => {
    setStarting(true);
    setError(undefined);
    try {
      await api.fixBoard(workspace, board);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setStarting(false);
    }
  };
  return (
    <section role="alert" aria-label={`${boardLabel} board problems`} className="board-problems mb-3 rounded-lg border border-destructive/25 bg-destructive/[0.04] p-3">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <span className="flex size-8 flex-none items-center justify-center rounded-full bg-destructive/10 text-destructive [&_svg]:size-4" aria-hidden>
          <IconAlert />
        </span>
        <div className="min-w-0 flex-1">
          <h2 className="text-[15px] font-semibold">
            {showWorkspace && <span className="text-muted-foreground">{workspace} · </span>}
            {boardLabel} has {problems.length === 1 ? "a problem" : `${problems.length} problems`}
          </h2>
          <p className="text-[13.5px] text-muted-foreground">Fix them yourself, or let an agent find the cause and fix it.</p>
        </div>
        {fixing ? (
          <span className="flex items-center gap-2 text-[13.5px] text-muted-foreground">
            <span className="toast-spinner !size-4" aria-hidden />
            {fixing.waiting ? "The agent needs your answer" : "An agent is fixing this"}
            <a className="inline-flex items-center gap-1 text-primary hover:underline" href={`/?terminal=${encodeURIComponent(agentTerminal(workspace, fixing.name))}`} target="_blank" rel="noreferrer">
              Open terminal <IconExternal />
            </a>
          </span>
        ) : (
          <Button variant="primary" onClick={fix} disabled={starting}>
            <IconBot />
            {starting ? "Starting…" : "Fix with agent"}
          </Button>
        )}
      </header>
      <ul className="mt-3 divide-y divide-destructive/15 overflow-hidden rounded-md border border-destructive/15 bg-card">
        {problems.map(({ stage, title, detail, places }) => (
          <li key={`${stage?.name ?? ""}|${title}|${detail}`} className="flex items-start gap-3 px-3 py-2">
            <Badge variant={stage ? "outline" : "danger"} className="mt-0.5 min-w-20 justify-center">
              {stage ? stage.label : "Board file"}
            </Badge>
            <div className="min-w-0">
              <p className="text-[14px] font-medium">{title}</p>
              {detail && <p className="font-mono text-xs break-all text-muted-foreground">{detail}</p>}
              {places.length > 0 && (
                <details className="text-xs text-muted-foreground">
                  <summary className="w-fit cursor-pointer select-none hover:text-foreground">
                    Looked in {places.length === 1 ? "1 place" : `${places.length} places`}
                  </summary>
                  <ol className="mt-1 list-decimal space-y-0.5 pl-5 font-mono break-all">
                    {places.map((place) => (
                      <li key={place}>{place}</li>
                    ))}
                  </ol>
                </details>
              )}
            </div>
          </li>
        ))}
      </ul>
      {error && <ErrorText className="mt-2">{error}</ErrorText>}
    </section>
  );
}
