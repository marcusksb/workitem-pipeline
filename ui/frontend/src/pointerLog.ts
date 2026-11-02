// Diagnostics for a Chrome state where hover stops and links need a double click.
// Every Workload tab appends its pointer, capture, drag and focus events to one
// log in localStorage. A mouse press far from the last mousemove the page saw
// means moves stopped arriving: that entry is marked `hover-lost` and logged to
// the console with what came before it. Run `workloadPointerLog()` in the
// console of any Workload tab to print the log.

export type Entry = {
  t: number;
  tab: string;
  type: string;
  x?: number;
  y?: number;
  target?: string;
  /** On `hover-lost`: pointer captures and drags this tab had not seen end. */
  open?: string[];
  /** On `pointerdown`: mouse moves the page received since the previous press. */
  moves?: number;
};

const KEY = "workload.pointerLog";
const LIMIT = 300;
/** Pixels a press may be away from the last move before hover counts as lost. */
const SLACK = 8;

export class PointerLog {
  private lastMove?: { x: number; y: number };
  private moves = 0;
  private open = new Set<string>();

  constructor(
    private tab: string,
    private load: () => Entry[],
    private save: (entries: Entry[]) => void,
    private now: () => number = Date.now,
  ) {}

  move(x: number, y: number) {
    this.lastMove = { x, y };
    this.moves++;
  }

  /** The window lost focus or the pointer left it; the next press needs no move first. */
  reset(type: string) {
    this.lastMove = undefined;
    this.add({ type });
  }

  press(x: number, y: number, target: string) {
    const last = this.lastMove;
    if (last && Math.hypot(x - last.x, y - last.y) > SLACK) {
      this.add({ type: "hover-lost", x, y, target, open: [...this.open] });
      console.warn("Workload: hover lost, recent pointer log:", this.load().slice(-40));
    }
    this.lastMove = { x, y };
    this.add({ type: "pointerdown", x, y, target, moves: this.moves });
    this.moves = 0;
  }

  /** Records an event; `opens`/`closes` name what it starts or ends (a capture, a drag). */
  event(type: string, target?: string, change?: { opens?: string; closes?: string }) {
    if (change?.opens) this.open.add(change.opens);
    if (change?.closes) this.open.delete(change.closes);
    this.add({ type, target });
  }

  private add(entry: Omit<Entry, "t" | "tab">) {
    this.save([...this.load(), { t: this.now(), tab: this.tab, ...entry }].slice(-LIMIT));
  }
}

function describe(target: EventTarget | null): string {
  if (!(target instanceof Element)) return String(target);
  const id = target.id ? `#${target.id}` : "";
  const classes = [...target.classList].slice(0, 2).map((c) => `.${c}`).join("");
  return `${target.tagName.toLowerCase()}${id}${classes}`;
}

export function installPointerLog() {
  const page = new URLSearchParams(location.search).get("terminal") ? "terminal" : "board";
  const tab = `${page}-${Math.random().toString(36).slice(2, 6)}`;
  const load = (): Entry[] => {
    try {
      return JSON.parse(localStorage.getItem(KEY) ?? "[]");
    } catch {
      return [];
    }
  };
  const save = (entries: Entry[]) => {
    try {
      localStorage.setItem(KEY, JSON.stringify(entries));
    } catch {
      // Storage full or blocked: monitoring is best effort.
    }
  };
  const log = new PointerLog(tab, load, save);
  (window as unknown as { workloadPointerLog: () => void }).workloadPointerLog = () => console.table(load());

  const options = { capture: true, passive: true };
  const mouse = (e: PointerEvent) => e.pointerType === "mouse";
  window.addEventListener("pointermove", (e) => mouse(e) && log.move(e.clientX, e.clientY), options);
  window.addEventListener("pointerdown", (e) => mouse(e) && log.press(e.clientX, e.clientY, describe(e.target)), options);
  window.addEventListener("pointerup", (e) => mouse(e) && log.event("pointerup", describe(e.target)), options);
  window.addEventListener("click", (e) => log.event("click", describe(e.target)), options);
  window.addEventListener("dblclick", (e) => log.event("dblclick", describe(e.target)), options);
  window.addEventListener("pointercancel", (e) => log.event("pointercancel", describe(e.target)), options);
  window.addEventListener("gotpointercapture", (e) =>
    log.event("gotpointercapture", describe(e.target), { opens: `capture:${e.pointerId}` }), options);
  window.addEventListener("lostpointercapture", (e) =>
    log.event("lostpointercapture", describe(e.target), { closes: `capture:${e.pointerId}` }), options);
  window.addEventListener("dragstart", (e) => log.event("dragstart", describe(e.target), { opens: "drag" }), options);
  window.addEventListener("dragend", (e) => log.event("dragend", describe(e.target), { closes: "drag" }), options);
  window.addEventListener("drop", (e) => log.event("drop", describe(e.target), { closes: "drag" }), options);
  document.documentElement.addEventListener("pointerleave", () => log.reset("pointerleave"));
  window.addEventListener("blur", () => log.reset("blur"));
  window.addEventListener("focus", () => log.event("focus"));
  document.addEventListener("visibilitychange", () => log.reset(`visibility:${document.visibilityState}`));
  log.event("load");
}
