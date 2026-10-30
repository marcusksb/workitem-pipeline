import { useMemo, useRef, useState } from "react";
import Markdown, { Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { api } from "./api";
import { toggleTask } from "./boardState";
import { Item } from "./types";

type Props = { item: Item; body: string; onSaved: () => void };

/** The rendered brief. Task list checkboxes are clickable and save at once; like the editor,
 * the save is refused if the file changed since `body` was read. */
export function BriefView({ item, body, onSaved }: Props) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const toggle = async (offset: number) => {
    const next = toggleTask(body, offset);
    if (next === undefined) return;
    setBusy(true);
    setError(undefined);
    try {
      await api.editBrief(item, body, next);
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  // The components must keep their identity across renders: new ones remount every list item,
  // and Chromium then scrolls the brief up. They read the current state from here.
  const current = useRef({ busy, toggle });
  current.current = { busy, toggle };
  const components = useMemo<Components>(
    () => ({
      li: ({ node, ...props }) => <li {...props} data-offset={node?.position?.start.offset} />,
      input: ({ node, ...props }) =>
        props.type === "checkbox" ? (
          <input
            {...props}
            disabled={current.current.busy}
            onChange={(e) => {
              const offset = e.currentTarget.closest("li")?.dataset.offset;
              if (offset !== undefined) void current.current.toggle(Number(offset));
            }}
          />
        ) : (
          <input {...props} />
        ),
    }),
    [],
  );

  return (
    <>
      {error && <p className="error">{error}</p>}
      <Markdown remarkPlugins={[remarkGfm]} components={components}>
        {body}
      </Markdown>
    </>
  );
}
