/** Hash routes: `#<workspace>/<dir>[/<tab>]` for the detail panel, `?workspace=<name>&board=<name>` for the workspace and the board
 * shown, `?plan=<workspace>/<id>` for an open planning session (instead of an item). `workspace=` is left out when the item or
 * plan already names it. */

export type Tab = "brief" | "info" | "files" | "pr" | "children" | "terminal" | "actions";
export type Route = { item?: { workspace: string; dir: string }; tab?: Tab; workspace?: string; board?: string; plan?: { workspace: string; id: string } };

const TABS: Tab[] = ["brief", "info", "files", "pr", "children", "terminal", "actions"];

/** Parses the location hash into a route; unknown or incomplete paths yield an empty route. */
export function readRoute(hash: string = location.hash): Route {
  const [path, query] = hash.replace(/^#/, "").split("?");
  const route: Route = {};
  const params = new URLSearchParams(query);
  const workspace = params.get("workspace");
  if (workspace) route.workspace = workspace;
  const board = params.get("board");
  if (board) route.board = board;
  const plan = params.get("plan");
  const slash = plan?.lastIndexOf("/") ?? -1;
  if (plan && slash > 0 && slash < plan.length - 1) route.plan = { workspace: plan.slice(0, slash), id: plan.slice(slash + 1) };
  const parts = path
    .split("/")
    .map(decodeURIComponent)
    .filter((part) => part !== "");
  if (parts.length < 2) return route;
  route.item = { workspace: parts[0], dir: parts[1] };
  if (TABS.includes(parts[2] as Tab)) route.tab = parts[2] as Tab;
  return route;
}

/** Serializes a route into a hash string; an empty route maps to no hash at all. */
export function routeHash(route: Route): string {
  const path = route.item
    ? [route.item.workspace, route.item.dir, ...(route.tab ? [route.tab] : [])].map(encodeURIComponent).join("/")
    : "";
  const implied = (route.item ?? route.plan)?.workspace === route.workspace;
  const params = [
    ...(route.workspace && !implied ? [`workspace=${encodeURIComponent(route.workspace)}`] : []),
    ...(route.board ? [`board=${encodeURIComponent(route.board)}`] : []),
    ...(route.plan ? [`plan=${encodeURIComponent(`${route.plan.workspace}/${route.plan.id}`)}`] : []),
  ];
  const query = params.length ? `?${params.join("&")}` : "";
  return path || query ? `#${path}${query}` : "";
}
