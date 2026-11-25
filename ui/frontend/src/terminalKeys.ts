type Keys = Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">;

// Keys the browser uses to switch tabs. xterm would turn them into terminal input and cancel them,
// so the terminal leaves them alone. Shift+PageUp/PageDown stay with the terminal for history scroll.
export function isBrowserTabShortcut(event: Keys): boolean {
  if (!event.ctrlKey || event.altKey || event.metaKey) return false;
  if (event.key === "Tab") return true;
  return !event.shiftKey && (event.key === "PageUp" || event.key === "PageDown");
}
