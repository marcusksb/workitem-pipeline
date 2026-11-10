import { describe, expect, it } from "vitest";
import { readRoute, routeHash } from "./url";

describe("readRoute", () => {
  it("parses item, item with tab, and percent-encoded segments", () => {
    expect(readRoute("#app/x-1")).toEqual({ item: { workspace: "app", dir: "x-1" } });
    expect(readRoute("#app/x-1/terminal")).toEqual({
      item: { workspace: "app", dir: "x-1" },
      tab: "terminal",
    });
    expect(readRoute("#my%20proj/a%2Fb/actions")).toEqual({
      item: { workspace: "my proj", dir: "a/b" },
      tab: "actions",
    });
  });

  it("reads the board, with or without an item", () => {
    expect(readRoute("#?board=research")).toEqual({ board: "research" });
    expect(readRoute("#app/x-1/pr?board=my%20board")).toEqual({ item: { workspace: "app", dir: "x-1" }, tab: "pr", board: "my board" });
  });

  it("reads the inbox and the chat tab, and writes them back", () => {
    const route = { item: { workspace: "app", dir: "x-1" }, tab: "chat" as const, inbox: true };
    expect(routeHash(route)).toBe("#app/x-1/chat?inbox=1");
    expect(readRoute(routeHash(route))).toEqual(route);
  });

  it("reads the workspace filter next to the board", () => {
    expect(readRoute("#?workspace=my%20proj")).toEqual({ workspace: "my proj" });
    expect(readRoute("#app/x-1?workspace=app&board=research")).toEqual({ item: { workspace: "app", dir: "x-1" }, workspace: "app", board: "research" });
  });

  it("yields an empty route for the empty, incomplete, or malformed hash", () => {
    expect(readRoute("")).toEqual({});
    expect(readRoute("#")).toEqual({});
    expect(readRoute("#only-workspace")).toEqual({});
    expect(readRoute("#app/x/nope")).toEqual({ item: { workspace: "app", dir: "x" } });
    expect(readRoute("#token=abc")).toEqual({});
  });
});

describe("routeHash", () => {
  it("round-trips routes and encodes special characters", () => {
    expect(routeHash({ item: { workspace: "app", dir: "x-1" } })).toBe("#app/x-1");
    expect(routeHash({ item: { workspace: "app", dir: "x-1" }, tab: "pr" })).toBe("#app/x-1/pr");
    const hash = routeHash({ item: { workspace: "my proj", dir: "a/b" }, tab: "brief" });
    expect(readRoute(hash)).toEqual({ item: { workspace: "my proj", dir: "a/b" }, tab: "brief" });
  });

  it("maps an empty route to no hash", () => {
    expect(routeHash({})).toBe("");
  });

  it("keeps the board with and without an item", () => {
    expect(routeHash({ board: "research" })).toBe("#?board=research");
    const route = { item: { workspace: "app", dir: "x-1" }, tab: "pr" as const, board: "my board" };
    expect(readRoute(routeHash(route))).toEqual(route);
  });

  it("keeps the workspace unless the item or plan already names it", () => {
    expect(routeHash({ workspace: "app" })).toBe("#?workspace=app");
    expect(routeHash({ item: { workspace: "a b", dir: "x-1" }, workspace: "a b", board: "research" })).toBe("#a%20b/x-1?board=research");
    expect(routeHash({ plan: { workspace: "app", id: "0a1b2c" }, workspace: "app" })).toBe("#?plan=app%2F0a1b2c");
    const route = { item: { workspace: "a b", dir: "x-1" }, workspace: "web", board: "research" };
    expect(readRoute(routeHash(route))).toEqual(route);
  });

  it("keeps an open plan, even when its workspace name has a slash", () => {
    const route = { workspace: "app", plan: { workspace: "team/app", id: "0a1b2c" } };
    expect(readRoute(routeHash(route))).toEqual(route);
    expect(readRoute("#?plan=app%2F")).toEqual({});
    expect(readRoute("#?plan=abc")).toEqual({});
  });
});
