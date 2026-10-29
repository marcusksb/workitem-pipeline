import { useEffect, useState } from "react";
import { api } from "./api";
import { AgentSessions } from "./AgentSessions";
import { Board } from "./Board";
import { itemKey, shownBoard } from "./boardState";
import { DetailPanel } from "./DetailPanel";
import { Login } from "./Login";
import { Projects } from "./Projects";
import { QuickSearch } from "./QuickSearch";
import { Terminal } from "./Terminal";
import { useBlockedAlerts } from "./useBlockedAlerts";
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

function Workspace({ onUnauthorized }: { onUnauthorized: () => void }) {
  const [route, setRoute] = useState<Route>(readRoute);
  const project = route.project ?? null;
  const projects = usePolling(api.projects, 3000, "projects", onUnauthorized);
  const pipelines = usePolling(api.pipelines, 3000, "pipelines", onUnauthorized);
  const items = usePolling(api.items, 3000, "items", onUnauthorized);
  const sessions = usePolling(api.sessions, 3000, "sessions", onUnauthorized);
  useBlockedAlerts(items.data);

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
    const hash = routeHash({ project: route.project, board: route.board, ...next });
    if (hash !== location.hash) {
      history.pushState(null, "", hash || location.pathname + location.search);
    }
    setRoute(readRoute());
  };

  const selected = route.item
    ? (items.data ?? []).find((i) => i.project === route.item!.project && i.dir === route.item!.dir) ?? null
    : null;
  const shown = (items.data ?? []).filter((item) => !project || item.project === project);
  const error = projects.error ?? items.error ?? pipelines.error;
  const inView = project ? [project] : (projects.data ?? []).map((p) => p.name);
  /** Shows a search result: on its board, and on all projects if the filtered one is not its own. */
  const openFound = (item: Item) => {
    setSearching(false);
    navigate({
      project: project === item.project ? project : undefined,
      board: shownBoard(item, pipelines.data),
      item: { project: item.project, dir: item.dir },
    });
  };
  const onTab = (tab: Tab) =>
    selected && navigate({ item: { project: selected.project, dir: selected.dir }, tab });

  return (
    <div className="app">
      <header className="top">
        <h1 className="brand"><img src="/favicon.svg" alt="" width={22} height={22} />Workload</h1>
        <Projects
          projects={projects.data ?? []}
          pipelines={pipelines.data}
          board={route.board}
          selected={project}
          onLaunched={() => void sessions.reload()}
          onSelect={(name) => navigate({ ...route, project: name ?? undefined })}
          onChanged={() => {
            void projects.reload();
            void items.reload();
          }}
        />
        <button className="search-open" onClick={() => setSearching(true)} title="Find a workitem by title, Jira key or PR number">
          Search <kbd>{/Mac|iPhone|iPad/.test(navigator.platform) ? "⌘" : "Ctrl+"}K</kbd>
        </button>
        {error && <span className="error">{error}</span>}
      </header>
      <AgentSessions sessions={sessions.data ?? []} />
      {projects.data?.length === 0 && <p className="empty">No projects yet. Add a repo directory to see its workitems.</p>}
      <main className={selected ? "with-detail" : ""}>
        <div className="left" onClick={(event) => selected && isBackground(event.target) && navigate({})}>
          <Board items={shown} pipelines={pipelines.data} projects={inView} board={route.board} onBoard={(board) => navigate({ ...route, board })} selected={selected && itemKey(selected)} onSelect={(item) => navigate({ item })} showProject={!project} onChanged={() => void items.reload()} />
        </div>
        {selected && (
          <DetailPanel
            item={selected}
            items={shown}
            onSelect={(item) => navigate({ item })}
            tab={route.tab}
            onTab={onTab}
            onClose={() => navigate({})}
            onUnauthorized={onUnauthorized}
          />
        )}
      </main>
      {searching && <QuickSearch items={items.data ?? []} showProject={(projects.data?.length ?? 0) > 1} onOpen={openFound} onClose={() => setSearching(false)} />}
    </div>
  );
}

export function App() {
  const session = useSession();
  if (session.state === "checking") return null;
  if (session.state === "out") return <Login onSubmit={session.login} error={session.error} />;
  return <Workspace onUnauthorized={session.expire} />;
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
