import { ReactNode, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { blockedByBoard, boardLabel, boardNames, itemsByBoard } from "./boardState";
import { IconBoard, IconBook, IconChevronDown, IconEye, IconPlus, IconSearch, IconSliders } from "./icons";
import { Learnings } from "./Learnings";
import { Item, Pipelines, Workspace } from "./types";
import { AddBoardDialog, AddWorkspaceDialog, SettingsDialog, trackerName, WatchersDialog } from "./Workspaces";

/** A full-width row of the sidebar. */
export const navItem = "h-[30px] w-full justify-start px-2 font-normal text-foreground hover:bg-foreground/5";
/** A section's small heading. */
export const navHeading = "px-2 py-1 text-[11.5px] font-medium text-muted-foreground";

const isMac = /Mac|iPhone|iPad/.test(navigator.platform);

type Props = {
  workspaces: Workspace[];
  /** The workspace on screen; there is always exactly one once any exists. */
  workspace: string | undefined;
  pipelines: Pipelines | undefined;
  /** The items of the workspace on screen. */
  items: Item[];
  /** The board on screen. */
  board: string | undefined;
  onWorkspace: (name: string) => void;
  onBoard: (name: string) => void;
  onSearch: () => void;
  /** Called after a workspace or board was added or changed. */
  onChanged: () => void;
  /** The Plans section. */
  children: ReactNode;
};

type Open = "workspace" | "board" | "settings" | "watchers" | "learnings";

/** The app's left column: workspace, search, boards, plans and the workspace's settings. */
export function Sidebar({ workspaces, workspace, pipelines, items, board, onWorkspace, onBoard, onSearch, onChanged, children }: Props) {
  const [open, setOpen] = useState<Open>();
  const close = () => setOpen(undefined);
  const current = workspaces.find((w) => w.name === workspace);
  const names = boardNames(pipelines, current ? [current.name] : []);
  const counts = itemsByBoard(items, pipelines);
  const blocked = blockedByBoard(items, pipelines);

  return (
    <nav className="sidebar flex w-[232px] shrink-0 flex-col gap-3.5 overflow-y-auto border-r border-border px-2.5 py-3 max-md:w-full max-md:overflow-visible max-md:border-r-0 max-md:border-b" aria-label="Workspace">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" className={cn(navItem, "h-8 gap-2 text-[13.5px] font-semibold")} aria-label={current ? `Workspace: ${current.name}` : "Workspace"}>
            <span className="flex size-5 items-center justify-center rounded-[5px] bg-primary text-[11px] font-semibold text-primary-foreground uppercase" aria-hidden="true">
              {current?.name[0] ?? "+"}
            </span>
            <span className="flex-1 truncate text-left">{current?.name ?? "No workspace"}</span>
            <IconChevronDown />
          </Button>
        </DropdownMenuTrigger>
        {/* A dialog opened from the menu keeps its focus. */}
        <DropdownMenuContent align="start" className="w-[212px]" onCloseAutoFocus={(event) => open && event.preventDefault()}>
          {workspaces.length > 0 && (
            <>
              <DropdownMenuRadioGroup value={workspace} onValueChange={onWorkspace}>
                {workspaces.map((w) => (
                  <DropdownMenuRadioItem key={w.name} value={w.name}>
                    {w.name}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
              <DropdownMenuSeparator />
            </>
          )}
          <DropdownMenuItem onSelect={() => setOpen("workspace")}>
            <IconPlus />
            Add workspace
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <Button variant="secondary" className="h-[30px] w-full justify-start gap-2 px-2 font-normal text-muted-foreground" onClick={onSearch} title="Find a workitem by title, issue key or PR number">
        <IconSearch />
        <span className="flex-1 text-left">Search</span>
        <kbd className="rounded border border-border px-1 font-sans text-[11px]">{isMac ? "⌘" : "Ctrl+"}K</kbd>
      </Button>

      {current && (
        <div className="flex flex-col gap-px">
          <div className={navHeading} id="boards-heading">
            Boards
          </div>
          <div role="tablist" aria-orientation="vertical" aria-labelledby="boards-heading" className="flex flex-col gap-px">
            {names.map((name) => {
              const repo = pipelines?.[current.name]?.find((b) => b.name === name)?.pipeline?.repo;
              const active = name === board;
              return (
                <Button
                  key={name}
                  variant="ghost"
                  role="tab"
                  aria-selected={active}
                  className={cn(navItem, "gap-2", active && "bg-foreground/[0.07] font-medium")}
                  title={repo}
                  onClick={() => onBoard(name)}
                >
                  <IconBoard />
                  <span className="flex-1 truncate text-left">{boardLabel(pipelines, current.name, name)}</span>
                  {blocked[name] ? (
                    <Badge variant="danger" title="blocked workitems">
                      {blocked[name]}
                    </Badge>
                  ) : null}
                  <span className="text-[11.5px] text-muted-foreground" title="workitems">
                    {counts[name] ?? 0}
                  </span>
                </Button>
              );
            })}
          </div>
          <Button variant="ghost" className={cn(navItem, "gap-2 text-muted-foreground")} title="Add a board for a repository" onClick={() => setOpen("board")}>
            <IconPlus />
            New board
          </Button>
        </div>
      )}

      {children}

      {current && (
        <div className="mt-auto flex flex-col gap-px">
          {board && (
            <Button variant="ghost" className={cn(navItem, "gap-2")} onClick={() => setOpen("learnings")}>
              <IconBook />
              Learnings
            </Button>
          )}
          <Button
            variant="ghost"
            className={cn(navItem, "gap-2", current.error && "text-destructive")}
            title={current.error ?? "Scripts that run once for the whole workspace, from workspace.toml"}
            onClick={() => setOpen("watchers")}
          >
            <IconEye />
            Watchers{current.error ? ": invalid workspace.toml" : ""}
          </Button>
          <Button
            variant="ghost"
            className={cn(navItem, "gap-2", current.account_error && "text-destructive")}
            title={current.account_error ? `Stopped: ${current.account_error}` : `Agent: ${current.agent || "default"} · Issues: ${trackerName(current.issue_tracker)}`}
            onClick={() => setOpen("settings")}
          >
            <IconSliders />
            Settings
          </Button>
        </div>
      )}

      {open === "workspace" && (
        <AddWorkspaceDialog
          onClose={close}
          onAdded={(name) => {
            close();
            onChanged();
            onWorkspace(name);
          }}
        />
      )}
      {open === "board" && current && (
        <AddBoardDialog
          workspace={current.name}
          onClose={close}
          onAdded={() => {
            close();
            onChanged();
          }}
        />
      )}
      {open === "settings" && current && (
        <SettingsDialog
          workspace={current}
          onClose={close}
          onSaved={() => {
            close();
            onChanged();
          }}
        />
      )}
      {open === "watchers" && current && <WatchersDialog workspace={current} onClose={close} />}
      {open === "learnings" && current && board && (
        <Learnings workspace={current.name} board={board} label={boardLabel(pipelines, current.name, board)} onClose={close} />
      )}
    </nav>
  );
}
