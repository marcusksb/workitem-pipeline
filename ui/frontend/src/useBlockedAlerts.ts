import { useEffect, useRef } from "react";
import { blockedKey, isBlocked, newlyBlocked } from "./boardState";
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

function beep() {
  if (!audio || audio.state !== "running") return;
  const osc = audio.createOscillator();
  const gain = audio.createGain();
  osc.frequency.value = 880;
  gain.gain.setValueAtTime(0.2, audio.currentTime);
  gain.gain.exponentialRampToValueAtTime(0.001, audio.currentTime + 0.4);
  osc.connect(gain).connect(audio.destination);
  osc.start();
  osc.stop(audio.currentTime + 0.4);
}

/** Notification and beep when an item becomes blocked. Items already blocked on first load stay quiet. */
export function useBlockedAlerts(items: Item[] | undefined) {
  const seen = useRef<Set<string>>(undefined);

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
    const fresh = seen.current ? newlyBlocked(seen.current, items) : [];
    seen.current = new Set(items.filter(isBlocked).map(blockedKey));
    if (fresh.length === 0) return;
    beep();
    if (typeof Notification === "undefined" || Notification.permission !== "granted") return;
    for (const item of fresh) {
      const note = new Notification(`Workitem blocked (${item.project})`, { body: `${item.dir}: ${item.agent_status}` });
      note.onclick = () => {
        window.focus();
        location.hash = routeHash({ item: { project: item.project, dir: item.dir } });
      };
    }
  }, [items]);
}
