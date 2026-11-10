import { FormEvent, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api, CHAT_KEYS, ChatKey, Unauthorized } from "./api";
import { chatRows, ToolCall } from "./chatState";
import { markdownLinks } from "./markdownLinks";
import { ChatEntry } from "./types";

type Props = {
  /** API base of the session: `itemChat(...)` or `planChat(...)`. */
  base: string;
  /** The session waits for a human, so the pending box is open. */
  blocked: boolean;
  onUnauthorized: () => void;
};

const KEY_LABELS: Record<ChatKey, string> = { "1": "1", "2": "2", "3": "3", Enter: "Enter", Escape: "Esc", Up: "↑", Down: "↓" };

/** The agent session as a conversation (decisions 58, 59): the transcript polled every 2s, a reply box,
 * and the bottom of the pane with key buttons for prompts the transcript does not show. */
export function Chat({ base, blocked, onUnauthorized }: Props) {
  const [entries, setEntries] = useState<ChatEntry[]>([]);
  const [session, setSession] = useState(false);
  const [screen, setScreen] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string>();
  const cursor = useRef({ next: 0, transcript: "" });
  /** Polls now, after a message or key, instead of on the next tick. */
  const poll = useRef(() => {});

  useEffect(() => {
    cursor.current = { next: 0, transcript: "" };
    setEntries([]);
    setLoaded(false);
    let stopped = false;
    let running = false;
    const tick = async () => {
      if (running) return;
      running = true;
      try {
        const chat = await api.chat(base, cursor.current.next, cursor.current.transcript);
        if (stopped) return;
        cursor.current = { next: chat.next, transcript: chat.transcript };
        setEntries((old) => (chat.reset ? chat.entries : chat.entries.length ? [...old, ...chat.entries] : old));
        setSession(chat.session);
        setScreen(chat.screen);
        setLoaded(true);
        setError(undefined);
      } catch (e) {
        if (stopped) return;
        if (e instanceof Unauthorized) onUnauthorized();
        else setError(e instanceof Error ? e.message : String(e));
      } finally {
        running = false;
      }
    };
    poll.current = () => void tick();
    void tick();
    const timer = setInterval(() => void tick(), 2000);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [base]);

  const rows = useMemo(() => chatRows(entries), [entries]);
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [rows, screen, blocked]);

  return (
    <div className="chat">
      <div
        className="chat-log"
        ref={scroller}
        onScroll={(event) => {
          const el = event.currentTarget;
          pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        }}
      >
        {loaded && rows.length === 0 && (
          <p className="chat-empty">{cursor.current.transcript ? "Nothing said yet." : "No transcript yet: sessions started before this version have none."}</p>
        )}
        {rows.map((row, i) =>
          row.kind === "tools" ? (
            <ToolGroup key={i} calls={row.calls} />
          ) : row.kind === "prompt" ? (
            <PromptBubble key={i} text={row.text} />
          ) : (
            <div key={i} className={`bubble ${row.kind}`}>
              {row.kind === "user" ? row.text : <Markdown remarkPlugins={[remarkGfm]} components={markdownLinks}>{row.text}</Markdown>}
            </div>
          ),
        )}
        {session && screen !== null && <Pending base={base} screen={screen} open={blocked} onPressed={() => poll.current()} />}
      </div>
      {error && <p className="error">{error}</p>}
      {session ? (
        <Reply base={base} onSent={() => {
          pinned.current = true;
          poll.current();
        }} />
      ) : (
        loaded && <p className="chat-ended">Session ended.</p>
      )}
    </div>
  );
}

/** The launcher's prompt (handler skill and stage block, or the planning rules): a few lines, and a tap shows all of it. */
function PromptBubble({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={`bubble user prompt${open ? " open" : ""}`}>
      <div className="prompt-text">
        <Markdown remarkPlugins={[remarkGfm]} components={markdownLinks}>{text}</Markdown>
      </div>
      <button className="link prompt-toggle" aria-expanded={open} onClick={() => setOpen(!open)}>
        {open ? "Show less" : "Show the whole prompt"}
      </button>
    </div>
  );
}

/** A run of tool calls: one line per call when there are few, else a summary line that opens them. */
function ToolGroup({ calls }: { calls: ToolCall[] }) {
  const lines = calls.map((call) => <ToolLine key={call.id} call={call} />);
  if (calls.length <= 2) return <div className="tool-group">{lines}</div>;
  const failed = calls.filter((c) => c.result?.error).length;
  return (
    <details className="tool-group">
      <summary>
        {calls.length} tool calls{failed ? <span className="tool-failed"> · {failed} failed</span> : null}
      </summary>
      {lines}
    </details>
  );
}

function ToolLine({ call }: { call: ToolCall }) {
  const status = !call.result ? "running" : call.result.error ? "failed" : "done";
  return (
    <details className={`tool ${status}`}>
      <summary>
        <span className="tool-name">{call.name}</span>
        <span className="tool-summary">{call.summary}</span>
      </summary>
      {call.result ? (
        <pre>
          {call.result.output || "(no output)"}
          {call.result.more > 0 && `\n… ${call.result.more} more lines`}
        </pre>
      ) : (
        <p className="tool-pending">Running…</p>
      )}
    </details>
  );
}

/** The bottom of the pane and the keys a prompt needs. Open while the session waits for you. */
function Pending({ base, screen, open, onPressed }: { base: string; screen: string; open: boolean; onPressed: () => void }) {
  const [error, setError] = useState<string>();
  const press = async (key: ChatKey) => {
    try {
      await api.pressKey(base, key);
      setError(undefined);
      onPressed();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  return (
    <details className={`pending${open ? " waiting" : ""}`} open={open || undefined}>
      <summary>{open ? "Waiting for you" : "Screen and keys"}</summary>
      {screen && <pre className="pending-screen">{screen}</pre>}
      <div className="pending-keys">
        {CHAT_KEYS.map((key) => (
          <button key={key} onClick={() => void press(key)}>
            {KEY_LABELS[key]}
          </button>
        ))}
      </div>
      {error && <p className="error">{error}</p>}
    </details>
  );
}

/** Sends a message to the session. Enter adds a line; the button, or ⌘/Ctrl+Enter, sends. */
function Reply({ base, onSent }: { base: string; onSent: () => void }) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const send = async (event?: FormEvent) => {
    event?.preventDefault();
    if (!text.trim() || busy) return;
    setBusy(true);
    try {
      await api.sendChat(base, text);
      setText("");
      setError(undefined);
      onSent();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <form className="chat-reply" onSubmit={(event) => void send(event)}>
      <textarea
        value={text}
        rows={Math.min(6, Math.max(1, text.split("\n").length))}
        placeholder="Message the agent"
        onChange={(event) => setText(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) void send();
        }}
      />
      <button type="submit" disabled={busy || !text.trim()} aria-label="Send">
        Send
      </button>
      {error && <p className="error">{error}</p>}
    </form>
  );
}
