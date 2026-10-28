import { useEffect, useState } from "react";
import { Unauthorized } from "./api";

/** Calls `load` now and every `ms`; `key` restarts it. Unauthorized is passed to `onUnauthorized`. */
export function usePolling<T>(load: () => Promise<T>, ms: number, key: string, onUnauthorized: () => void) {
  const [data, setData] = useState<T>();
  const [error, setError] = useState<string>();

  useEffect(() => {
    let stopped = false;
    const tick = () =>
      load().then(
        (value) => {
          if (stopped) return;
          setData(value);
          setError(undefined);
        },
        (e: unknown) => {
          if (stopped) return;
          if (e instanceof Unauthorized) onUnauthorized();
          else setError(e instanceof Error ? e.message : String(e));
        },
      );
    tick();
    const timer = setInterval(tick, ms);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, ms]);

  return { data, error, reload: () => load().then(setData) };
}
