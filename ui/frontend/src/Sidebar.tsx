import { ReactNode, useRef, useState, useSyncExternalStore } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { blockedByBoard, boardColors, boardLabel, boardNames, itemsByBoard } from "./boardState";
import { IconBoard, IconBook, IconChevronDown, IconEye, IconPanelLeft, IconPointer, IconPlus, IconSliders } from "./icons";
import { Learnings } from "./Learnings";
import { Item, Pipelines, Workspace } from "./types";
import { AddBoardDialog, AddWorkspaceDialog, SettingsDialog, trackerName, WatchersDialog } from "./Workspaces";

/** A full-width row of the sidebar; only its icon in the rail. */
export const navItem =
  "h-[30px] w-full justify-start px-2 font-normal text-foreground hover:bg-foreground/5 group-data-[collapsed=true]/sidebar:justify-center group-data-[collapsed=true]/sidebar:px-0";
/** A section's small heading. */
export const navHeading = "px-2 py-1 text-[11.5px] font-medium text-muted-foreground";
/** Text that the rail keeps for screen readers only. */
export const navLabel = "group-data-[collapsed=true]/sidebar:sr-only";
/** Parts the rail leaves out. */
export const navExtra = "group-data-[collapsed=true]/sidebar:hidden";

/** Below this width the sidebar is a rail unless it was expanded by hand. */
const AUTO_COLLAPSE = "(max-width: 1099px)";
/** Below this width an expanded sidebar floats over the content (`max-md`). */
const OVERLAY = "(max-width: 767px)";

/** "1" while the sidebar is a rail that expands on hover; kept per browser. */
const HOVER_KEY = "workload.sidebar.hover";

function loadHover(): boolean {
  try {
    return localStorage.getItem(HOVER_KEY) === "1";
  } catch {
    return false;
  }
}

function saveHover(hover: boolean) {
  try {
    localStorage.setItem(HOVER_KEY, hover ? "1" : "0");
  } catch {
  }
}

function useMedia(query: string): boolean {
  return useSyncExternalStore(
    (onChange) => {
      const list = matchMedia(query);
      list.addEventListener("change", onChange);
      return () => list.removeEventListener("change", onChange);
    },
    () => matchMedia(query).matches,
  );
}


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
  /** Called after a workspace or board was added or changed. */
  onChanged: () => void;
  /** The Plans section. */
  children: ReactNode;
};

type Open = "workspace" | "board" | "settings" | "watchers" | "learnings";

/** The app's left column: workspace, boards, plans and the workspace's settings. */
export function Sidebar({ workspaces, workspace, pipelines, items, board, onWorkspace, onBoard, onChanged, children }: Props) {
  const [open, setOpen] = useState<Open>();
  const close = () => setOpen(undefined);
  const current = workspaces.find((w) => w.name === workspace);
  const names = boardNames(pipelines, current ? [current.name] : []);
  const counts = itemsByBoard(items, pipelines);
  const blocked = blockedByBoard(items, pipelines);
  const colors = boardColors(names);
  // The width decides until the toggle is used; the choice holds until the width crosses AUTO_COLLAPSE again.
  const narrow = useMedia(AUTO_COLLAPSE);
  const overlay = useMedia(OVERLAY);
  const [choice, setChoice] = useState<{ narrow: boolean; collapsed: boolean }>();
  const pinnedCollapsed = choice?.narrow === narrow ? choice.collapsed : narrow;
  const collapse = (value: boolean) => setChoice({ narrow, collapsed: value });
  // Expand on hover: a rail that floats open while the pointer or focus is in it, or a menu or dialog
  // of it is open. Phones have no hover, so it does not apply there.
  const [hoverMode, setHoverMode] = useState(loadHover);
  const [hovered, setHovered] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const leaveTimer = useRef<number>(undefined);
  const hover = hoverMode && !overlay;
  const collapsed = hover ? !(hovered || menuOpen || open) : pinnedCollapsed;
  const phoneOpen = overlay && !pinnedCollapsed;
  const floating = hover || phoneOpen;
  const enter = () => {
    window.clearTimeout(leaveTimer.current);
    setHovered(true);
  };
  const leave = () => {
    window.clearTimeout(leaveTimer.current);
    leaveTimer.current = window.setTimeout(() => setHovered(false), 250);
  };
  const toggleHover = () => {
    setHoverMode(!hoverMode);
    saveHover(!hoverMode);
    // The button is under the pointer (or focused) now, so the sidebar stays open either way.
    if (hoverMode) collapse(false);
    else setHovered(true);
  };

  return (
    <nav className="flex bg-[color-mix(in_srgb,var(--muted)_50%,var(--background))] w-[232px] shrink-0 flex-col gap-3.5 overflow-y-auto border-r border-border px-2.5 py-3 max-md:w-full max-md:overflow-visible max-md:border-r-0 max-md:border-b" aria-label="Workspace">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" className={cn(navItem, "h-8 gap-2 text-[13.5px] font-semibold")} aria-label={current ? `Workspace: ${current.name}` : "Workspace"}>
            <span className="flex size-5 items-center justify-center rounded-[5px] bg-primary text-[11px] font-semibold text-primary-foreground uppercase" aria-hidden="true">
              {current?.name[0] ?? "+"}
            </span>
            <span className={cn("flex-1 truncate text-left", navLabel)}>{current?.name ?? "No workspace"}</span>
            <span className={navExtra}>
              <IconChevronDown />
            </span>
          </Button>
        </DropdownMenuTrigger>
        {/* A dialog opened from the menu keeps its focus. */}
        <DropdownMenuContent align="start" className="w-[240px]" onCloseAutoFocus={(event) => open && event.preventDefault()}>
          {workspaces.length > 0 && (
            <>
              <DropdownMenuRadioGroup value={workspace} onValueChange={(name) => name !== workspace && onWorkspace(name)}>
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

      {current && (
        <div className="flex flex-col gap-px">
          <div className={cn(navHeading, navLabel)} id="boards-heading">
            Boards
          </div>
          <div role="tablist" aria-orientation="vertical" aria-labelledby="boards-heading" className="flex flex-col gap-px">
            {names.map((name) => {
              const repo = pipelines?.[current.name]?.find((b) => b.name === name)?.pipeline?.repo;
              const active = name === board;
              const label = boardLabel(pipelines, current.name, name);
              const color = colors[name];
              return (
                <Button
                  key={name}
                  variant="ghost"
                  role="tab"
                  aria-selected={active}
                  className={cn(navItem, "gap-2", active && "bg-foreground/[0.07] font-medium")}
                  title={[label, repo, blocked[name] && `${blocked[name]} blocked`].filter(Boolean).join("\n")}
                  onClick={() => {
                    onBoard(name);
                    if (overlay) collapse(true);
                  }}
                >
                  <span className="relative flex size-5 shrink-0 items-center justify-center rounded-[5px]" style={{ color, backgroundColor: `color-mix(in srgb, ${color} 16%, transparent)` }}>
                    <IconBoard />
                    {blocked[name] ? <span className="absolute -top-0.5 -right-0.5 hidden size-2 rounded-full bg-destructive ring-2 ring-background group-data-[collapsed=true]/sidebar:block" /> : null}
                  </span>
                  <span className={cn("flex-1 truncate text-left", navLabel)}>{label}</span>
                  {blocked[name] ? (
                    <Badge variant="danger" className={navExtra} title="blocked workitems">
                      {blocked[name]}
                    </Badge>
                  ) : null}
                  <span className={cn("text-[11.5px] text-muted-foreground", navExtra)} title="workitems">
                    {counts[name] ?? 0}
                  </span>
                </Button>
              );
            })}
          </div>
          <Button variant="ghost" className={cn(navItem, "gap-2 text-muted-foreground")} title="Add a board for a repository" onClick={() => setOpen("board")}>
            <IconPlus />
            <span className={navLabel}>New board</span>
          </Button>
        </div>
      )}

      {children}

      <div className="mt-auto flex flex-col gap-px">
      {current && (
        <>
          {board && (
            <Button variant="ghost" className={cn(navItem, "gap-2")} title="What agents learned on this board" onClick={() => setOpen("learnings")}>
              <IconBook />
              <span className={navLabel}>Learnings</span>
            </Button>
          )}
          <Button
            variant="ghost"
            className={cn(navItem, "gap-2", current.error && "text-destructive")}
            title={current.error ?? "Scripts that run once for the whole workspace, from workspace.toml"}
            onClick={() => setOpen("watchers")}
          >
            <IconEye />
            <span className={navLabel}>Watchers{current.error ? ": invalid workspace.toml" : ""}</span>
          </Button>
          <Button
            variant="ghost"
            className={cn(navItem, "gap-2", current.account_error && "text-destructive")}
            title={current.account_error ? `Stopped: ${current.account_error}` : `Agent: ${current.agent || "default"} · Issues: ${trackerName(current.issue_tracker)}`}
            onClick={() => setOpen("settings")}
          >
            <IconSliders />
            <span className={navLabel}>Settings</span>
          </Button>
        </>
      )}
        {/* Expand on hover decides the width, so the toggle stays in place but does nothing. */}
        <Button variant="ghost" className={cn(navItem, "gap-2 text-muted-foreground")} title={collapsed ? "Expand sidebar" : "Collapse sidebar"} aria-expanded={!collapsed} disabled={hover} onClick={() => collapse(!collapsed)}>
          <IconPanelLeft />
          <span className={navLabel}>{collapsed ? "Expand sidebar" : "Collapse sidebar"}</span>
        </Button>
        {!overlay && (
          <Button
            variant="ghost"
            className={cn(navItem, "gap-2 text-muted-foreground", hoverMode && "bg-foreground/[0.07] text-foreground")}
            title={hoverMode ? "Keep the sidebar open" : "Collapse the sidebar and open it while the pointer is on it"}
            aria-pressed={hoverMode}
            onClick={toggleHover}
          >
            <IconPointer />
            <span className={navLabel}>Expand on hover</span>
          </Button>
        )}
      </div>

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
    </>
  );
}
