import { useEffect, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { api } from "./api";
import { Dialog } from "./Dialog";
import { markdownLinks } from "./markdownLinks";
import { Learnings as Data } from "./types";

type Props = { project: string; onClose: () => void };

/** A project's `learnings.md`, read-only: the rules review fixers wrote down. Edit the file to prune it. */
export function Learnings({ project, onClose }: Props) {
  const [data, setData] = useState<Data>();
  const [error, setError] = useState<string>();

  useEffect(() => {
    api.learnings(project).then(setData, (e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, [project]);

  return (
    <Dialog title={`Learnings in ${project}`} onClose={onClose}>
      <div className="source">
        {data && (
          <p className="source-path">
            <code title={data.path}>{data.path}</code>
          </p>
        )}
        {error && <p className="error">{error}</p>}
        {!data && !error && <p className="empty">Loading…</p>}
        {data && !data.content.trim() && <p className="empty">No learnings yet. Review fixers add them when a comment applies beyond its PR.</p>}
        {data && data.content.trim() && (
          <div className="file-markdown learnings-body">
            <Markdown remarkPlugins={[remarkGfm]} components={markdownLinks}>{data.content}</Markdown>
          </div>
        )}
      </div>
    </Dialog>
  );
}
