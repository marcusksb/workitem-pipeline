import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal as XTerm } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef, useState } from "react";
import { terminalUrl } from "./api";

type Scrollback = { history: number; position: number };

/** xterm.js attached to the item's tmux session. Unmounting detaches; the agent keeps running.
 *  tmux repaints in place, so xterm has no scrollback; the scrollbar drives tmux's history instead. */
export function Terminal({ path }: { path: string }) {
  const container = useRef<HTMLDivElement>(null);
  const track = useRef<HTMLDivElement>(null);
  const scrollTo = useRef<(line: number) => void>(() => {});
  const [scrollback, setScrollback] = useState<Scrollback>({ history: 0, position: 0 });
  const [rows, setRows] = useState(24);

  useEffect(() => {
    const term = new XTerm({ cursorBlink: true, fontFamily: "Menlo, monospace", fontSize: 13, scrollback: 0 });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());
    term.open(container.current!);
    fit.fit();
    setRows(term.rows);

    const socket = new WebSocket(terminalUrl(path, term.cols, term.rows));
    socket.binaryType = "arraybuffer";
    const encoder = new TextEncoder();
    const sendSize = () => {
      if (socket.readyState === WebSocket.OPEN) {
        socket.send(JSON.stringify({ cols: term.cols, rows: term.rows }));
      }
    };
    let latest: Scrollback = { history: 0, position: 0 };
    let frame = 0;
    let exact = 0; // position with the fraction that small trackpad deltas add up to
    let lastLocal = 0;
    scrollTo.current = (line) => {
      exact = Math.max(0, Math.min(latest.history, line));
      lastLocal = Date.now();
      const position = Math.round(exact);
      if (position === latest.position) return;
      latest = { ...latest, position };
      setScrollback(latest);
      // At most one request per frame; wheel and drag events arrive faster than tmux can follow.
      if (frame === 0) {
        frame = requestAnimationFrame(() => {
          frame = 0;
          if (socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ scroll: latest.position }));
        });
      }
    };
    socket.onmessage = (event) => {
      if (typeof event.data === "string") {
        const next = JSON.parse(event.data) as Scrollback;
        // The poll can report a position older than the one just requested; keep ours meanwhile.
        if (Date.now() - lastLocal < 500) next.position = latest.position;
        else exact = next.position;
        latest = next;
        setScrollback(latest);
      } else {
        term.write(new Uint8Array(event.data as ArrayBuffer));
      }
    };
    const lineHeight = () => container.current!.clientHeight / term.rows;
    term.attachCustomWheelEventHandler((event) => {
      const lines = event.deltaMode === 0 ? event.deltaY / lineHeight() : event.deltaMode === 1 ? event.deltaY : event.deltaY * term.rows;
      scrollTo.current(exact - lines);
      return false;
    });
    // Shift+PageUp/PageDown scroll history; plain keys still go to the agent.
    term.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown" || !event.shiftKey || (event.key !== "PageUp" && event.key !== "PageDown")) return true;
      scrollTo.current(exact + (event.key === "PageUp" ? 1 : -1) * Math.max(1, term.rows - 1));
      return false;
    });
    let touchY: number | null = null;
    const screen = container.current!;
    const onTouchStart = (event: TouchEvent) => (touchY = event.touches[0].clientY);
    const onTouchMove = (event: TouchEvent) => {
      if (touchY === null) return;
      const y = event.touches[0].clientY;
      scrollTo.current(exact + (y - touchY) / lineHeight());
      touchY = y;
      event.preventDefault();
    };
    screen.addEventListener("touchstart", onTouchStart, { passive: true });
    screen.addEventListener("touchmove", onTouchMove, { passive: false });
    socket.onclose = () => term.write("\r\n[disconnected]\r\n");
    term.onData((data) => {
      if (socket.readyState === WebSocket.OPEN) socket.send(encoder.encode(data));
    });
    const observer = new ResizeObserver(() => {
      fit.fit();
      setRows(term.rows);
      sendSize();
    });
    observer.observe(container.current!);
    term.focus();

    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      screen.removeEventListener("touchstart", onTouchStart);
      screen.removeEventListener("touchmove", onTouchMove);
      socket.close();
      term.dispose();
    };
  }, [path]);

  // Thumb size and offset as fractions of the track; position 0 is the live bottom.
  const total = scrollback.history + rows;
  const thumb = rows / total;
  const top = (1 - scrollback.position / Math.max(1, scrollback.history)) * (1 - thumb);
  const dragTo = (event: React.PointerEvent) => {
    const box = track.current!.getBoundingClientRect();
    const fraction = (event.clientY - box.top - (thumb * box.height) / 2) / (box.height * (1 - thumb));
    scrollTo.current((1 - Math.max(0, Math.min(1, fraction))) * scrollback.history);
  };

  return (
    <div className="terminal">
      <div className="terminal-screen" ref={container} />
      {scrollback.history > 0 && (
        <div
          className="terminal-scrollbar"
          ref={track}
          onPointerDown={(event) => {
            event.currentTarget.setPointerCapture(event.pointerId);
            dragTo(event);
          }}
          onPointerMove={(event) => event.buttons === 1 && dragTo(event)}
        >
          <div className="terminal-thumb" style={{ height: `${thumb * 100}%`, top: `${top * 100}%` }} />
        </div>
      )}
    </div>
  );
}
