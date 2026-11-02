import { describe, expect, it, vi } from "vitest";
import { type Entry, PointerLog } from "./pointerLog";

function setup() {
  let entries: Entry[] = [];
  const log = new PointerLog("board-x", () => entries, (e) => (entries = e), () => 1);
  vi.spyOn(console, "warn").mockImplementation(() => {});
  return { log, types: () => entries.map((e) => e.type), entries: () => entries };
}

describe("PointerLog", () => {
  it("accepts a press where the last move was", () => {
    const { log, types } = setup();
    log.move(100, 100);
    log.press(102, 101, "a");
    expect(types()).toEqual(["pointerdown"]);
  });

  it("counts the moves between presses", () => {
    const { log, entries } = setup();
    log.move(1, 1);
    log.move(2, 2);
    log.press(2, 2, "a");
    log.press(2, 2, "a");
    expect(entries().map((e) => e.moves)).toEqual([2, 0]);
  });

  it("flags a press far from the last move as hover-lost, with open captures", () => {
    const { log, types, entries } = setup();
    log.event("gotpointercapture", "div", { opens: "capture:1" });
    log.move(100, 100);
    log.press(400, 300, "a");
    expect(types()).toEqual(["gotpointercapture", "hover-lost", "pointerdown"]);
    expect(entries()[1].open).toEqual(["capture:1"]);
  });

  it("does not flag after the pointer left the window or it lost focus", () => {
    const { log, types } = setup();
    log.move(100, 100);
    log.reset("blur");
    log.press(400, 300, "a");
    expect(types()).toEqual(["blur", "pointerdown"]);
  });

  it("closes what an event ends", () => {
    const { log, entries } = setup();
    log.event("dragstart", "a", { opens: "drag" });
    log.event("dragend", "a", { closes: "drag" });
    log.move(0, 0);
    log.press(500, 500, "a");
    expect(entries().find((e) => e.type === "hover-lost")?.open).toEqual([]);
  });
});
