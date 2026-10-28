import { useState } from "react";

/** Runs one action at a time, reports its outcome, and calls `onDone` afterwards. */
export function useRun(onDone: () => void) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string>();
  const [error, setError] = useState<string>();

  const run = async (label: string, action: () => Promise<unknown>, confirmText?: string) => {
    if (confirmText && !confirm(confirmText)) return;
    setBusy(true);
    setError(undefined);
    try {
      await action();
      setMessage(`${label}: done`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      onDone();
    }
  };

  return { busy, message, error, run };
}
