import { ReactNode, useEffect, useReducer, useRef, useState } from "react";
import { api } from "./api";
import { Board } from "./Board";
import { boardNames, itemKey, shownBoard } from "./boardState";
import { DetailPanel } from "./DetailPanel";
import { ActivityToasts } from "./ActivityToasts";
import { Login } from "./Login";
import { PlanPanel } from "./PlanPanel";
import { Plans } from "./Plans";
import { Workspaces } from "./Workspaces";
import { QuickSearch } from "./QuickSearch";
import { Terminal } from "./Terminal";
import { useAlerts } from "./useAlerts";
import { usePolling } from "./usePolling";
import { Item } from "./types";
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
  if (target.closest(".card, button, a, input, select, textarea, label, .dialog-overlay")) return false;
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
    <div className={leaving ? "panel-slot leaving" : "panel-slot"} onAnimationEnd={(event) => event.animationName === "detail-out" && onDone()}>
      {children}
    </div>
  );
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
  /** Opens an item in its workspace, switching to its board only when another one is on screen. */
  const open = (item: Item) => {
    const board = shownBoard(item, pipelines.data);
    const sameView = item.workspace === workspace && board === activeBoard;
    navigate({ workspace: item.workspace, board: sameView ? route.board : board, item: { workspace: item.workspace, dir: item.dir } });
  };
  useAlerts(items.data, open);
  const openFound = (item: Item) => {
    setSearching(false);
    open(item);
  };
  const onTab = (tab: Tab) =>
    selected && navigate({ item: { workspace: selected.workspace, dir: selected.dir }, tab });

  return (
    <div className="app">
      <header className="top">
        <h1 className="brand"><img src="/favicon.svg" alt="" width={22} height={22} />Workload</h1>
        <Workspaces
          workspaces={workspaces.data ?? []}
          pipelines={pipelines.data}
          board={route.board}
          selected={workspace}
          onLaunched={() => void sessions.reload()}
          onSelect={(name) => navigate({ workspace: name, board: undefined })}
          onChanged={() => {
            void workspaces.reload();
            void items.reload();
          }}
        />
        <button className="search-open" onClick={() => setSearching(true)} title="Find a workitem by title, Jira key or PR number">
          Search <kbd>{/Mac|iPhone|iPad/.test(navigator.platform) ? "⌘" : "Ctrl+"}K</kbd>
        </button>
        {error && <span className="error">{error}</span>}
      </header>
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
      />
      {workspaces.data?.length === 0 && <p className="empty">No workspaces yet. Add a workspace, then a board for each repository.</p>}
      {workspace && pipelines.data && (pipelines.data[workspace] ?? []).length === 0 && (
        <p className="empty">No boards in {workspace} yet. Add one for a repository with + Board.</p>
      )}
      <main>
        <div className="left" onClick={(event) => detailOpen && isBackground(event.target) && navigate({})}>
          <Board items={shown} pipelines={pipelines.data} workspaces={inView} board={route.board} onBoard={(board) => navigate({ ...route, board })} selected={selected && itemKey(selected)} onSelect={open} showWorkspace={false} onChanged={() => {
            void items.reload();
            void pipelines.reload();
          }} />
        </div>
        {itemPanel.shown && (
          <Leaving leaving={itemPanel.leaving} onDone={itemPanel.done}>
            <DetailPanel
              item={itemPanel.shown}
              items={shown}
              pipelines={pipelines.data}
              onSelect={open}
              tab={route.tab}
              onTab={onTab}
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
              onWorkitem={(itemWorkspace, dir) => {
                const item = (items.data ?? []).find((i) => i.workspace === itemWorkspace && i.dir === dir);
                if (item) open(item);
              }}
              onClose={() => navigate({})}
              onChanged={() => void plans.reload()}
              onUnauthorized={onUnauthorized}
            />
          </Leaving>
        )}
      </main>
      <ActivityToasts items={items.data} pipelines={pipelines.data} onOpen={open} />
      {searching && <QuickSearch items={items.data ?? []} showWorkspace={(workspaces.data?.length ?? 0) > 1} onOpen={openFound} onClose={() => setSearching(false)} />}
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
    <div className="popout">
      <div className="popout-title">{decodeURIComponent(path.replace(/^(terminal|session)\//, ""))}</div>
      <Terminal path={path} />
    </div>
  );
}
