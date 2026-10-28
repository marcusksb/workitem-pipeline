import { describe, expect, it } from "vitest";
import { readRoute, routeHash } from "./url";

describe("readRoute", () => {
  it("parses item, item with tab, and percent-encoded segments", () => {
    expect(readRoute("#app/x-1")).toEqual({ item: { project: "app", dir: "x-1" } });
    expect(readRoute("#app/x-1/terminal")).toEqual({
      item: { project: "app", dir: "x-1" },
      tab: "terminal",
    });
    expect(readRoute("#my%20proj/a%2Fb/actions")).toEqual({
      item: { project: "my proj", dir: "a/b" },
      tab: "actions",
    });
  });

  it("reads the board, with or without an item", () => {
    expect(readRoute("#?board=research")).toEqual({ board: "research" });
    expect(readRoute("#app/x-1/pr?board=my%20board")).toEqual({ item: { project: "app", dir: "x-1" }, tab: "pr", board: "my board" });
  });

  it("reads the project filter next to the board", () => {
    expect(readRoute("#?project=my%20proj")).toEqual({ project: "my proj" });
    expect(readRoute("#app/x-1?project=app&board=research")).toEqual({ item: { project: "app", dir: "x-1" }, project: "app", board: "research" });
  });

  it("yields an empty route for the empty, incomplete, or malformed hash", () => {
    expect(readRoute("")).toEqual({});
    expect(readRoute("#")).toEqual({});
    expect(readRoute("#only-project")).toEqual({});
    expect(readRoute("#app/x/nope")).toEqual({ item: { project: "app", dir: "x" } });
    expect(readRoute("#token=abc")).toEqual({});
  });
});

describe("routeHash", () => {
  it("round-trips routes and encodes special characters", () => {
    expect(routeHash({ item: { project: "app", dir: "x-1" } })).toBe("#app/x-1");
    expect(routeHash({ item: { project: "app", dir: "x-1" }, tab: "pr" })).toBe("#app/x-1/pr");
    const hash = routeHash({ item: { project: "my proj", dir: "a/b" }, tab: "brief" });
    expect(readRoute(hash)).toEqual({ item: { project: "my proj", dir: "a/b" }, tab: "brief" });
  });

  it("maps an empty route to no hash", () => {
    expect(routeHash({})).toBe("");
  });

  it("keeps the board with and without an item", () => {
    expect(routeHash({ board: "research" })).toBe("#?board=research");
    const route = { item: { project: "app", dir: "x-1" }, tab: "pr" as const, board: "my board" };
    expect(readRoute(routeHash(route))).toEqual(route);
  });

  it("keeps the project filter", () => {
    expect(routeHash({ project: "app" })).toBe("#?project=app");
    const route = { item: { project: "a b", dir: "x-1" }, project: "a b", board: "research" };
    expect(readRoute(routeHash(route))).toEqual(route);
  });
});
