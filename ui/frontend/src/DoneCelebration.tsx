import confetti from "canvas-confetti";
import { useEffect, useRef, useState } from "react";
import { itemKey, newlyDone } from "./boardState";
import { Item, Pipelines } from "./types";
import { routeHash } from "./url";

const SHOWN = 3;

/** Confetti and a toast when items move into a final stage. Items already there on first load stay quiet. */
export function DoneCelebration({ items, pipelines }: { items: Item[] | undefined; pipelines: Pipelines | undefined }) {
  const seen = useRef<Map<string, string>>(new Map());
  const [done, setDone] = useState<Item[]>([]);
  const [hovered, setHovered] = useState(false);

  useEffect(() => {
    if (!items) return;
    const fresh = newlyDone(seen.current, items, pipelines);
    seen.current = new Map(items.map((item) => [itemKey(item), item.stage]));
    if (fresh.length === 0) return;
    void confetti({ particleCount: 160, spread: 100, startVelocity: 60, origin: { x: 0.5, y: 1 }, disableForReducedMotion: true });
    setDone((shown) => [...shown, ...fresh]);
  }, [items, pipelines]);

  useEffect(() => {
    if (done.length === 0 || hovered) return;
    const timer = setTimeout(() => setDone([]), 4000);
    return () => clearTimeout(timer);
  }, [done, hovered]);

  if (done.length === 0) return null;
  const open = (item: Item) => {
    setDone([]);
    setHovered(false);
    location.hash = routeHash({ item: { project: item.project, dir: item.dir } });
  };
  return (
    <div className="done-toast" role="status" onMouseEnter={() => setHovered(true)} onMouseLeave={() => setHovered(false)}>
      {done.slice(0, SHOWN).map((item) => (
        <button key={itemKey(item)} type="button" onClick={() => open(item)}>
          <span className="done-title">{item.title} is done</span>
          <span className="done-project">{item.project}</span>
        </button>
      ))}
      {done.length > SHOWN && <div className="done-more">+{done.length - SHOWN} more</div>}
    </div>
  );
}
