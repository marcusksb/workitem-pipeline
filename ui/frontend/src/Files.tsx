import { useEffect, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api, fileUrl, Unauthorized } from "./api";
import { FileEntry, Item } from "./types";
import { usePolling } from "./usePolling";
import { IconExternal, IconFolder } from "./icons";

const IMAGE = /\.(png|jpe?g|gif|webp|svg|avif|ico)$/i;
const VIDEO = /\.(mp4|webm|mov|m4v|ogv)$/i;
const MARKDOWN = /\.(md|markdown)$/i;
/** Larger files are only offered to open, not previewed. */
const MAX_PREVIEW = 1024 * 1024;

const sizeLabel = (size: number) =>
  size < 1024 ? `${size} B` : size < 1024 * 1024 ? `${(size / 1024).toFixed(1)} KB` : `${(size / 1024 / 1024).toFixed(1)} MB`;

type Text = { text: string } | { binary: true } | { error: string };

/** Fetches a text file again whenever its size changes; a NUL byte marks it as binary. */
function useText(url: string | undefined, size: number | undefined, onUnauthorized: () => void) {
  const [text, setText] = useState<Text>();
  useEffect(() => {
    setText(undefined);
    if (!url) return;
    let stopped = false;
    fetch(url)
      .then(async (response) => {
        if (response.status === 401) throw new Unauthorized();
        if (!response.ok) throw new Error(response.statusText);
        const body = await response.text();
        return body.includes("\0") ? { binary: true as const } : { text: body };
      })
      .then(
        (value) => !stopped && setText(value),
        (e: unknown) => {
          if (stopped) return;
          if (e instanceof Unauthorized) onUnauthorized();
          else setText({ error: e instanceof Error ? e.message : String(e) });
        },
      );
    return () => {
      stopped = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [url, size]);
  return text;
}

function Preview({ item, file, onUnauthorized }: { item: Item; file: FileEntry; onUnauthorized: () => void }) {
  const url = fileUrl(item, file.path);
  const image = IMAGE.test(file.path);
  const video = VIDEO.test(file.path);
  const tooBig = file.size > MAX_PREVIEW;
  const text = useText(image || video || tooBig ? undefined : url, file.size, onUnauthorized);
  const [revealError, setRevealError] = useState<string>();
  useEffect(() => setRevealError(undefined), [url]);
  const reveal = () =>
    api.reveal(item, file.path).then(
      () => setRevealError(undefined),
      (e: unknown) => (e instanceof Unauthorized ? onUnauthorized() : setRevealError(e instanceof Error ? e.message : String(e))),
    );
  return (
    <div className="file-preview">
      <p className="source-path">
        <code>{file.path}</code>
        <span className="badge">{sizeLabel(file.size)}</span>
        <span className="file-open">
          <button className="link" onClick={() => void reveal()}>
            <IconFolder />
            Show in Finder
          </button>
          <a className="link" href={url} target="_blank" rel="noreferrer">
            <IconExternal />
            Open
          </a>
        </span>
      </p>
      {revealError && <p className="error">{revealError}</p>}
      {image ? (
        <img src={url} alt={file.path} />
      ) : video ? (
        <video src={url} controls />
      ) : tooBig ? (
        <p className="empty">Too large to preview; open it instead.</p>
      ) : !text ? (
        <p className="empty">Loading…</p>
      ) : "error" in text ? (
        <p className="error">{text.error}</p>
      ) : "binary" in text ? (
        <p className="empty">Binary file; open it instead.</p>
      ) : MARKDOWN.test(file.path) ? (
        <div className="file-markdown">
          <Markdown remarkPlugins={[remarkGfm]}>{text.text}</Markdown>
        </div>
      ) : (
        <pre>{text.text}</pre>
      )}
    </div>
  );
}

/** The files of the workitem folder: a list, and a preview of the one picked. */
export function Files({ item, onUnauthorized }: { item: Item; onUnauthorized: () => void }) {
  const key = `${item.project}/${item.dir}`;
  const { data: files, error } = usePolling(() => api.files(item), 3000, key, onUnauthorized);
  const [picked, setPicked] = useState<string>();
  useEffect(() => setPicked(undefined), [key]);
  const file = files?.find((f) => f.path === picked);

  return (
    <div className="brief files">
      {error && <p className="error">{error}</p>}
      {!files && !error && <p className="empty">Loading…</p>}
      {files && files.length === 0 && <p className="empty">No files.</p>}
      {files && files.length > 0 && (
        <ul className="file-list">
          {files.map((f) => (
            <li key={f.path}>
              <button className={f.path === picked ? "active" : ""} onClick={() => setPicked(f.path)}>
                <span>{f.path}</span>
                <small>{sizeLabel(f.size)}</small>
              </button>
            </li>
          ))}
        </ul>
      )}
      {file && <Preview item={item} file={file} onUnauthorized={onUnauthorized} />}
    </div>
  );
}
