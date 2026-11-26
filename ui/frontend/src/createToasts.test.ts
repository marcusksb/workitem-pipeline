import { describe, expect, it } from "vitest";
import { CreateToast, killsOnClose, nextCreateToasts } from "./createToasts";
import { item } from "./testFixtures";
import { AgentSession } from "./types";

const session = (over: Partial<AgentSession> = {}): AgentSession => ({ workspace: "app", name: "wi-new-1", waiting: false, dead: false, ...over });
const states = (toasts: CreateToast[]) => toasts.map((t) => t.state);

describe("nextCreateToasts", () => {
  it("shows a running session, and an amber one while it waits for an answer", () => {
    const running = nextCreateToasts([], [session()], [], true);
    expect(states(running)).toEqual(["running"]);
    const waiting = nextCreateToasts(running, [session({ waiting: true })], [], false);
    expect(states(waiting)).toEqual(["waiting"]);
    expect(states(nextCreateToasts(waiting, [session()], [], false))).toEqual(["running"]);
  });

  it("links the items a gone session wrote, one per split child", () => {
    const running = nextCreateToasts([], [session()], [], true);
    const items = [item({ dir: "a", created_by: "wi-new-1" }), item({ dir: "b", created_by: "wi-new-1" }), item({ dir: "c", created_by: "wi-new-2" }), item({ dir: "d", workspace: "web", created_by: "wi-new-1" })];
    const [created] = nextCreateToasts(running, [], items, true);
    expect(created.state).toBe("created");
    expect(created.items.map((i) => i.dir)).toEqual(["a", "b"]);
  });

  it("ends a gone session without items after two item polls, not before", () => {
    let toasts = nextCreateToasts([], [session()], [], true);
    toasts = nextCreateToasts(toasts, [], [], true);
    expect(states(toasts)).toEqual(["running"]);
    toasts = nextCreateToasts(toasts, [], [], false);
    expect(states(toasts)).toEqual(["running"]);
    toasts = nextCreateToasts(toasts, [], [], true);
    expect(states(toasts)).toEqual(["ended"]);
    expect(states(nextCreateToasts(toasts, [], [item({ created_by: "wi-new-1" })], true))).toEqual(["ended"]);
  });

  it("ends a dead session, also one found on the first poll", () => {
    expect(states(nextCreateToasts([], [session({ dead: true })], [], true))).toEqual(["ended"]);
    const running = nextCreateToasts([], [session()], [], true);
    expect(states(nextCreateToasts(running, [session({ dead: true })], [], false))).toEqual(["ended"]);
  });

  it("keeps a session of another workspace apart", () => {
    const toasts = nextCreateToasts([], [session(), session({ workspace: "web", waiting: true })], [], true);
    expect(toasts.map((t) => [t.session.workspace, t.state])).toEqual([["app", "running"], ["web", "waiting"]]);
  });
});

describe("killsOnClose", () => {
  const toast = (state: CreateToast["state"], dead: boolean): CreateToast => ({ session: session({ dead }), state, items: [], misses: 0 });

  it("kills only an ended toast whose session died", () => {
    expect(killsOnClose(toast("ended", true))).toBe(true);
    expect(killsOnClose(toast("ended", false))).toBe(false);
    for (const state of ["running", "waiting", "created"] as const) expect(killsOnClose(toast(state, false))).toBe(false);
  });
});
