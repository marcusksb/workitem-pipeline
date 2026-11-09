import { useEffect, useRef } from "react";
import { blockedKey, isBlocked, issueDone, itemKey, newlyBlocked, newlyMatching, readyToMerge } from "./boardState";
import { Item } from "./types";
import { routeHash } from "./url";

let audio: AudioContext | undefined;

/** Browsers only allow sound and the permission prompt after a click, so arm both on the first one. */
function arm() {
  audio ??= new AudioContext();
  void audio.resume();
  if (typeof Notification !== "undefined" && Notification.permission === "default") {
    void Notification.requestPermission();
  }
}

/** Plays `notes` (Hz) one after another, `step` seconds apart. */
function play(notes: number[], step = 0.4) {
  const ctx = audio;
  if (!ctx || ctx.state !== "running") return;
  notes.forEach((frequency, i) => {
    const start = ctx.currentTime + i * step;
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.frequency.value = frequency;
    gain.gain.setValueAtTime(0.2, start);
    gain.gain.exponentialRampToValueAtTime(0.001, start + 0.4);
    osc.connect(gain).connect(ctx.destination);
    osc.start(start);
    osc.stop(start + 0.4);
  });
}

const beep = () => play([880]);
/** Rising fourth: a PR waits only for the merge button. */
const chime = () => play([659, 880], 0.12);
/** Major arpeggio: the Jira issue is done. */
const fanfare = () => play([523, 659, 784, 1047], 0.1);

const matching = (items: Item[], matches: (item: Item) => boolean) => new Set(items.filter(matches).map(itemKey));

/**
 * Notification and beep when an item becomes blocked; a chime when its PR becomes ready to merge, a fanfare
 * when its Jira issue becomes done. Items already in that state on first load stay quiet.
 */
export function useAlerts(items: Item[] | undefined) {
  const seen = useRef<Set<string>>(undefined);
  const ready = useRef<Set<string>>(undefined);
  const done = useRef<Set<string>>(undefined);

  useEffect(() => {
    window.addEventListener("pointerdown", arm, { once: true });
    window.addEventListener("keydown", arm, { once: true });
    return () => {
      window.removeEventListener("pointerdown", arm);
      window.removeEventListener("keydown", arm);
    };
  }, []);

  useEffect(() => {
    if (!items) return;
    if (ready.current && newlyMatching(ready.current, items, readyToMerge).length > 0) chime();
    if (done.current && newlyMatching(done.current, items, issueDone).length > 0) fanfare();
    ready.current = matching(items, readyToMerge);
    done.current = matching(items, issueDone);
  }, [items]);

  useEffect(() => {
    if (!items) return;
    const fresh = seen.current ? newlyBlocked(seen.current, items) : [];
    seen.current = new Set(items.filter(isBlocked).map(blockedKey));
    if (fresh.length === 0) return;
    beep();
    if (typeof Notification === "undefined" || Notification.permission !== "granted") return;
    for (const item of fresh) {
      const note = new Notification(`Workitem blocked (${item.workspace})`, { body: `${item.dir}: ${item.agent_status}` });
      note.onclick = () => {
        window.focus();
        location.hash = routeHash({ item: { workspace: item.workspace, dir: item.dir } });
      };
    }
  }, [items]);
}
