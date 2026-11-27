import { useEffect, useState } from "react";
import { useMedia } from "./useMedia";

/** A theme the user picked; none picked means the page follows the OS. Kept per browser.
 * index.html applies the stored one before the first paint, with its own copy of the key. */
export type Theme = "light" | "dark";

const THEME_KEY = "workload.theme";

/** Calls `onChange` when another tab of this browser changes the stored theme. */
function onOtherTab(onChange: () => void) {
  const listener = (event: StorageEvent) => {
    if (event.key === THEME_KEY || event.key === null) onChange();
  };
  window.addEventListener("storage", listener);
  return () => window.removeEventListener("storage", listener);
}

/** Keeps data-theme in step with a toggle in another tab, on every page (the terminal page too). */
export function followOtherTabs() {
  onOtherTab(() => applyTheme(storedTheme()));
}

export function storedTheme(): Theme | undefined {
  try {
    const theme = localStorage.getItem(THEME_KEY);
    return theme === "light" || theme === "dark" ? theme : undefined;
  } catch {
    return undefined;
  }
}

/** Sets data-theme on <html>, which styles.css turns into its color-scheme. */
export function applyTheme(theme: Theme | undefined) {
  if (theme) document.documentElement.dataset.theme = theme;
  else delete document.documentElement.dataset.theme;
}

/** The theme a toggle switches to: the opposite of the one on screen. */
export function nextTheme(current: Theme | undefined, osDark: boolean): Theme {
  return (current ?? (osDark ? "dark" : "light")) === "dark" ? "light" : "dark";
}

/** The theme on screen, and a toggle that picks the other one and remembers it. */
export function useTheme(): { dark: boolean; toggle: () => void } {
  const [theme, setTheme] = useState(storedTheme);
  const osDark = useMedia("(prefers-color-scheme: dark)");
  useEffect(() => onOtherTab(() => setTheme(storedTheme())), []);
  const toggle = () => {
    const next = nextTheme(theme, osDark);
    setTheme(next);
    applyTheme(next);
    try {
      localStorage.setItem(THEME_KEY, next);
    } catch {
      // the choice just is not remembered
    }
  };
  return { dark: (theme ?? (osDark ? "dark" : "light")) === "dark", toggle };
}
