import { ReactNode, useEffect, useReducer, useRef, useState } from "react";
import { api } from "./api";
import { Board } from "./Board";
import { Action, boardNames, itemKey, shownBoard } from "./boardState";
import { DetailPanel } from "./DetailPanel";
import { ActivityToasts } from "./ActivityToasts";
import { Login } from "./Login";
import { PlanPanel } from "./PlanPanel";
import { Plans } from "./Plans";
import { QuickSearch } from "./QuickSearch";
import { Sidebar } from "./Sidebar";
import { Terminal } from "./Terminal";
import { TopBar, View } from "./TopBar";
import { useAlerts } from "./useAlerts";
import { usePolling } from "./usePolling";
import { Item, Plan } from "./types";
import { readRoute, Route, routeHash, Tab } from "./url";

/** Exchanges `#token=…` from the printed URL for the session cookie, then strips it from the address bar. */
async function signInFromUrl(): Promise<void> {
  const token = new URLSearchParams(location.hash.slice(1)).get("token");
  if (!token) return;
  await api.login(token);
  history.replaceState(null, "", location.pathname + location.search);
}

export function useSession() {
  const [state, setState] = useState<"checking" | "in" | "out">("checking");
  const [error, setError] = useState<string>();

  useEffect(() => {
    signInFromUrl()
      .then(() => api.me())
      .then(
        () => setState("in"),
        () => setState("out"),
      );
  }, []);

  const login = (token: string) =>
    api.login(token).then(
      () => {
        setError(undefined);
        setState("in");
      },
      (e: unknown) => setError(e instanceof Error ? e.message : String(e)),
    );

  return { state, error, login, expire: () => setState("out") };
}

/** A click on the board that hits no card, control or dialog, and selects no text; it closes the open item. */
function isBackground(target: EventTarget): boolean {
  if (!(target instanceof Element)) return false;
  if (target.closest(".card, button, a, input, select, textarea, label, [role=dialog], [data-slot=dialog-overlay]")) return false;
  return window.getSelection()?.isCollapsed ?? true;
}

/** Keeps the last non-null value on screen after it goes away, until `done` is called (when its slide-out ends). */
function useLeaving<T>(value: T | null) {
  const last = useRef<T | null>(null);
  const [, rerender] = useReducer((n: number) => n + 1, 0);
  if (value) last.current = value;
  const done = () => {
    last.current = null;
    rerender();
  };
  return { shown: last.current, leaving: !value && !!last.current, done };
}

/** Wraps a `.detail` panel so it slides out before it unmounts. */
function Leaving({ leaving, onDone, children }: { leaving: boolean; onDone: () => void; children: ReactNode }) {
  return (
    <div className={leaving ? "contents leaving" : "contents"} onAnimationEnd={(event) => event.animationName === "detail-out" && onDone()}>
      {children}
    </div>
  );
}

const VIEW_KEY = "workload.view";

function storedView(): View {
  try {
    return localStorage.getItem(VIEW_KEY) === "list" ? "list" : "board";
  } catch {
    return "board";
  }
}

function Main({ onUnauthorized }: { onUnauthorized: () => void }) {
  const [route, setRoute] = useState<Route>(readRoute);
  const workspaces = usePolling(api.workspaces, 3000, "workspaces", onUnauthorized);
  const names = (workspaces.data ?? []).map((w) => w.name);
  // Exactly one workspace is on screen: the one asked for, else the open item's or plan's, else the first.
  const wanted = route.workspace ?? route.item?.workspace ?? route.plan?.workspace;
  const workspace: string | undefined = wanted && names.includes(wanted) ? wanted : names[0];
  const pipelines = usePolling(api.pipelines, 3000, "pipelines", onUnauthorized);
  const items = usePolling(api.items, 3000, "items", onUnauthorized);
  const sessions = usePolling(api.sessions, 3000, "sessions", onUnauthorized);
  const plans = usePolling(api.plans, 3000, "plans", onUnauthorized);

  useEffect(() => {
    const onHash = () => setRoute(readRoute());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const [view, setView] = useState<View>(storedView);
  const chooseView = (next: View) => {
    setView(next);
    try {
      localStorage.setItem(VIEW_KEY, next);
    } catch {
      // the choice just is not remembered
    }
  };

  const [searching, setSearching] = useState(false);
  useEffect(() => {
    // Capture phase, so a focused terminal does not swallow the shortcut.
    const onKey = (event: KeyboardEvent) => {
      if (event.key.toLowerCase() === "k" && (event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey) {
        event.preventDefault();
        event.stopPropagation();
        setSearching((open) => !open);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  /** Navigates to a route, leaving a history entry so Back and Forward walk items and tabs. */
  const navigate = (next: Route) => {
    const hash = routeHash({ workspace, board: route.board, ...next });
    if (hash !== location.hash) {
      history.pushState(null, "", hash || location.pathname + location.search);
    }
    setRoute(readRoute());
  };

  const selected = route.item
    ? (items.data ?? []).find((i) => i.workspace === route.item!.workspace && i.dir === route.item!.dir) ?? null
    : null;
  const selectedPlan = route.plan
    ? (plans.data ?? []).find((p) => p.workspace === route.plan!.workspace && p.id === route.plan!.id) ?? null
    : null;
  const detailOpen = !!(selected || selectedPlan);
  const itemPanel = useLeaving(selected);
  const planPanel = useLeaving(selectedPlan);
  const shown = (items.data ?? []).filter((item) => item.workspace === workspace);
  const error = workspaces.error ?? items.error ?? pipelines.error;
  const inView = workspace ? [workspace] : [];
  const boardsInView = boardNames(pipelines.data, inView);
  const activeBoard = route.board && boardsInView.includes(route.board) ? route.board : boardsInView[0];
  /** Opens an item in its workspace, switching to its board only when another one is on screen
   * and its parent is not on this one (children are shown under their parent too). */
  const open = (item: Item, tab?: Tab) => {
    const board = shownBoard(item, pipelines.data);
    const underParent = shown.some((p) => p.dir === item.parent && shownBoard(p, pipelines.data) === activeBoard);
    const sameView = item.workspace === workspace && (board === activeBoard || underParent);
    navigate({ workspace: item.workspace, board: sameView ? route.board : board, item: { workspace: item.workspace, dir: item.dir }, tab });
  };
  useAlerts(items.data, open);
  /** The workitems made from a plan that still exist. */
  const planWorkitems = (plan: Plan) => (items.data ?? []).filter((i) => i.workspace === plan.workspace && plan.workitems.includes(i.dir));
  const openFound = (item: Item) => {
    setSearching(false);
    open(item);
  };
  const [creatingItem, setCreatingItem] = useState(false);
  const [creatingPlan, setCreatingPlan] = useState(false);
  /** What the palette can run, on the same conditions as the buttons it stands in for. */
  const actions: Action[] = [
    ...(workspace && activeBoard ? [{ label: "New workitem", run: () => setCreatingItem(true) }] : []),
    ...(inView.length ? [{ label: "New plan", run: () => setCreatingPlan(true) }] : []),
    ...names.filter((name) => name !== workspace).map((name) => ({ label: `Switch to workspace ${name}`, run: () => navigate({ workspace: name, board: undefined }) })),
  ];
  const onTab = (tab: Tab) =>
    selected && navigate({ item: { workspace: selected.workspace, dir: selected.dir }, tab });

  return (
    <div className="flex h-screen">
      <Sidebar
        workspaces={workspaces.data ?? []}
        workspace={workspace}
        pipelines={pipelines.data}
        items={shown}
        board={activeBoard}
        onWorkspace={(name) => navigate({ workspace: name, board: undefined })}
        onBoard={(board) => navigate({ ...route, board })}
        onChanged={() => {
          void workspaces.reload();
          void items.reload();
          void pipelines.reload();
        }}
      >
        <Plans
          plans={plans.data ?? []}
          sessions={sessions.data ?? []}
          workspaces={inView}
          showWorkspace={false}
          selected={selectedPlan}
          onSelect={(plan) => navigate({ plan: { workspace: plan.workspace, id: plan.id } })}
          onStarted={(planWorkspace, id) => {
            void plans.reload();
            navigate({ plan: { workspace: planWorkspace, id } });
          }}
          creating={creatingPlan}
          onCreating={setCreatingPlan}
        />
      </Sidebar>
      <div className="content flex min-w-0 flex-1 flex-col">
        <TopBar
          workspace={workspaces.data?.find((w) => w.name === workspace)}
          pipelines={pipelines.data}
          board={activeBoard}
          view={view}
          onView={chooseView}
          onSearch={() => setSearching(true)}
          error={error}
          creating={creatingItem}
          onCreating={setCreatingItem}
          onCreated={() => void sessions.reload()}
          onBoardChanged={() => void pipelines.reload()}
        />
        {workspaces.data?.length === 0 && <p className="px-6 pt-4 text-[15px] text-muted-foreground">No workspaces yet. Add a workspace, then a board for each repository.</p>}
        {workspace && pipelines.data && (pipelines.data[workspace] ?? []).length === 0 && (
          <p className="px-6 pt-4 text-[15px] text-muted-foreground">No boards in {workspace} yet. Add one for a repository with New board.</p>
        )}
        <main className="relative flex min-h-0 flex-1">
          <div className="min-w-0 flex-1 overflow-auto px-6 py-4 max-md:px-4" onClick={(event) => detailOpen && isBackground(event.target) && navigate({})}>
            <Board items={shown} pipelines={pipelines.data} workspaces={inView} board={route.board} view={view} selected={selected && itemKey(selected)} onSelect={open} showWorkspace={false} onChanged={() => {
              void items.reload();
              void pipelines.reload();
            }} />
          </div>
          {itemPanel.shown && (
            <Leaving leaving={itemPanel.leaving} onDone={itemPanel.done}>
              <DetailPanel
                key={`${itemPanel.shown.workspace}/${itemPanel.shown.dir}`}
                item={itemPanel.shown}
                items={shown}
                pipelines={pipelines.data}
                onSelect={open}
                tab={route.tab}
                onTab={onTab}
                onBoard={(board) => navigate({ ...route, board })}
                onClose={() => navigate({})}
                onUnauthorized={onUnauthorized}
              />
            </Leaving>
          )}
          {planPanel.shown && (
            <Leaving leaving={planPanel.leaving} onDone={planPanel.done}>
              <PlanPanel
                key={`${planPanel.shown.workspace}/${planPanel.shown.id}`}
                plan={planPanel.shown}
                workitems={planWorkitems(planPanel.shown)}
                onSelect={open}
                onItemsChanged={() => void items.reload()}
                onClose={() => navigate({})}
                onChanged={() => void plans.reload()}
                onUnauthorized={onUnauthorized}
              />
            </Leaving>
          )}
        </main>
      </div>
      <ActivityToasts items={items.data} pipelines={pipelines.data} onOpen={open} />
      {searching && <QuickSearch items={items.data ?? []} actions={actions} showWorkspace={(workspaces.data?.length ?? 0) > 1} onOpen={openFound} onClose={() => setSearching(false)} />}
    </div>
  );
}

export function App() {
  const session = useSession();
  if (session.state === "checking") return null;
  if (session.state === "out") return <Login onSubmit={session.login} error={session.error} />;
  return <Main onUnauthorized={session.expire} />;
}

export function TerminalPage({ path }: { path: string }) {
  const session = useSession();
  if (session.state === "checking") return null;
  if (session.state === "out") return <Login onSubmit={session.login} error={session.error} />;
  return (
    <div className="flex h-screen flex-col bg-black">
      <div className="px-2 py-1 font-mono text-[14px] text-[#ccc]">{decodeURIComponent(path.replace(/^(terminal|session)\//, ""))}</div>
      <Terminal path={path} />
    </div>
  );
}
