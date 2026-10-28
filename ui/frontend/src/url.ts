/** Hash routes: `#<project>/<dir>[/<tab>]` for the detail panel, `?project=<name>&board=<name>` for the project filter and the board shown. */

export type Tab = "brief" | "info" | "files" | "pr" | "terminal" | "actions";
export type Route = { item?: { project: string; dir: string }; tab?: Tab; project?: string; board?: string };

const TABS: Tab[] = ["brief", "info", "files", "pr", "terminal", "actions"];

/** Parses the location hash into a route; unknown or incomplete paths yield an empty route. */
export function readRoute(hash: string = location.hash): Route {
  const [path, query] = hash.replace(/^#/, "").split("?");
  const route: Route = {};
  const params = new URLSearchParams(query);
  const project = params.get("project");
  if (project) route.project = project;
  const board = params.get("board");
  if (board) route.board = board;
  const parts = path
    .split("/")
    .map(decodeURIComponent)
    .filter((part) => part !== "");
  if (parts.length < 2) return route;
  route.item = { project: parts[0], dir: parts[1] };
  if (TABS.includes(parts[2] as Tab)) route.tab = parts[2] as Tab;
  return route;
}

/** Serializes a route into a hash string; an empty route maps to no hash at all. */
export function routeHash(route: Route): string {
  const path = route.item
    ? [route.item.project, route.item.dir, ...(route.tab ? [route.tab] : [])].map(encodeURIComponent).join("/")
    : "";
  const params = [
    ...(route.project ? [`project=${encodeURIComponent(route.project)}`] : []),
    ...(route.board ? [`board=${encodeURIComponent(route.board)}`] : []),
  ];
  const query = params.length ? `?${params.join("&")}` : "";
  return path || query ? `#${path}${query}` : "";
}
