import { useState } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { boardLabel, boardOfWorkspace, repoName } from "./boardState";
import { IconPlus, IconSearch } from "./icons";
import { Pipelines, Workspace } from "./types";
import { NewWorkitemDialog } from "./Workspaces";

const isMac = /Mac|iPhone|iPad/.test(navigator.platform);

export type View = "board" | "list";

type Props = {
  workspace: Workspace | undefined;
  pipelines: Pipelines | undefined;
  /** The board on screen. */
  board: string | undefined;
  view: View;
  onView: (view: View) => void;
  onSearch: () => void;
  error: string | undefined;
  /** Called once a workitem session was started. */
  onCreated: () => void;
};

/** The bar above the board: which board and repo, Search, Board or List, and New workitem. */
export function TopBar({ workspace, pipelines, board, view, onView, onSearch, error, onCreated }: Props) {
  const [creating, setCreating] = useState(false);
  const [draft, setDraft] = useState("");
  const repo = workspace && board ? boardOfWorkspace(pipelines, workspace.name, board)?.pipeline?.repo : undefined;
  return (
    <header className="top-bar flex min-h-12 flex-wrap items-center gap-x-3 gap-y-2 border-b border-border bg-card px-5 py-2">
      <div className="flex min-w-0 flex-[1_1_160px] items-baseline gap-2">
        <h1 className="truncate text-sm font-semibold">{workspace && board ? boardLabel(pipelines, workspace.name, board) : (workspace?.name ?? "Workload")}</h1>
        {repo && (
          <span className="truncate font-mono text-[13.5px] text-muted-foreground" title={repo}>
            {repoName(repo)}
          </span>
        )}
      </div>
      {error && <span className="text-[15px] text-destructive">{error}</span>}
      <Button variant="secondary" className="h-[30px] gap-2 px-2 font-normal text-muted-foreground" onClick={onSearch} title="Find a workitem by title, issue key or PR number">
        <IconSearch />
        Search
        <kbd className="rounded border border-border px-1 font-sans text-[13px]">{isMac ? "⌘" : "Ctrl+"}K</kbd>
      </Button>
      {board && (
        <div role="group" aria-label="View" className="flex rounded-[7px] bg-muted p-0.5">
          {(["board", "list"] as const).map((v) => (
            <Button
              key={v}
              variant="ghost"
              aria-pressed={view === v}
              className={cn("h-[26px] px-2.5 capitalize hover:bg-transparent", view === v && "bg-card text-foreground shadow-xs hover:bg-card")}
              onClick={() => onView(v)}
            >
              {v}
            </Button>
          ))}
        </div>
      )}
      {workspace && board && (
        <Button variant="primary" className="h-[30px] px-3" onClick={() => setCreating(true)}>
          <IconPlus />
          New workitem
        </Button>
      )}
      {creating && workspace && board && (
        <NewWorkitemDialog
          workspace={workspace}
          pipelines={pipelines}
          board={board}
          input={draft}
          onInput={setDraft}
          onClose={() => setCreating(false)}
          onCreated={() => {
            setCreating(false);
            setDraft("");
            onCreated();
          }}
        />
      )}
    </header>
  );
}
