import { useEffect, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api, fileUrl, Unauthorized } from "./api";
import { markdownLinks } from "./markdownLinks";
import { FileEntry, Item } from "./types";
import { usePolling } from "./usePolling";
import { IconExternal, IconFolder } from "./icons";
import { ErrorText, Muted, prose, scroll } from "./parts";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

const linkClass = "inline-flex cursor-pointer items-center gap-1 text-[15px] text-primary no-underline hover:underline";

const IMAGE = /\.(png|jpe?g|gif|webp|svg|avif|ico)$/i;
const VIDEO = /\.(mp4|webm|mov|m4v|ogv)$/i;
const MARKDOWN = /\.(md|markdown)$/i;
const HTML = /\.html?$/i;
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
  const html = HTML.test(file.path);
  const tooBig = file.size > MAX_PREVIEW;
  const text = useText(image || video || html || tooBig ? undefined : url, file.size, onUnauthorized);
  const [revealError, setRevealError] = useState<string>();
  useEffect(() => setRevealError(undefined), [url]);
  const reveal = () =>
    api.reveal(item, file.path).then(
      () => setRevealError(undefined),
      (e: unknown) => (e instanceof Unauthorized ? onUnauthorized() : setRevealError(e instanceof Error ? e.message : String(e))),
    );
  return (
    <div className="file-preview grid gap-2">
      <p className="flex flex-wrap items-center gap-2">
        <code className="font-mono text-[14px] [overflow-wrap:anywhere] text-muted-foreground">{file.path}</code>
        <Badge>{sizeLabel(file.size)}</Badge>
        <span className="ml-auto inline-flex gap-3">
          <button className={linkClass} onClick={() => void reveal()}>
            <IconFolder />
            Show in Finder
          </button>
          <a className={linkClass} href={url} target="_blank" rel="noreferrer">
            <IconExternal />
            Open
          </a>
        </span>
      </p>
      {revealError && <ErrorText>{revealError}</ErrorText>}
      {image ? (
        <img className="max-w-full rounded border border-border" src={url} alt={file.path} />
      ) : video ? (
        <video className="max-w-full rounded border border-border" src={url} controls />
      ) : tooBig ? (
        <Muted>Too large to preview; open it instead.</Muted>
      ) : html ? (
        <iframe className="h-[70vh] w-full rounded border border-border bg-white" sandbox="allow-scripts" src={url} title={file.path} />
      ) : !text ? (
        <Muted>Loading…</Muted>
      ) : "error" in text ? (
        <ErrorText>{text.error}</ErrorText>
      ) : "binary" in text ? (
        <Muted>Binary file; open it instead.</Muted>
      ) : MARKDOWN.test(file.path) ? (
        <div className={cn(prose, "border-t border-border pt-2")}>
          <Markdown remarkPlugins={[remarkGfm]} components={markdownLinks}>{text.text}</Markdown>
        </div>
      ) : (
        <pre className="overflow-auto rounded-md border border-border bg-muted px-3 py-2.5 font-mono text-[14px] whitespace-pre">{text.text}</pre>
      )}
    </div>
  );
}

/** The files of the workitem folder: a list, and a preview of the one picked. */
export function Files({ item, onUnauthorized }: { item: Item; onUnauthorized: () => void }) {
  const key = `${item.workspace}/${item.dir}`;
  const { data: files, error } = usePolling(() => api.files(item), 3000, key, onUnauthorized);
  const [picked, setPicked] = useState<string>();
  useEffect(() => setPicked(undefined), [key]);
  const file = files?.find((f) => f.path === picked);

  return (
    <div className={cn("grid content-start gap-3", scroll)}>
      {error && <ErrorText>{error}</ErrorText>}
      {!files && !error && <Muted>Loading…</Muted>}
      {files && files.length === 0 && <Muted>No files.</Muted>}
      {files && files.length > 0 && (
        <ul className="file-list overflow-hidden rounded-lg border border-border">
          {files.map((f) => (
            <li key={f.path} className="border-border not-first:border-t">
              <button
                className={cn(
                  "flex w-full cursor-pointer justify-between gap-3 px-2.5 py-1 text-left font-mono text-[14px] hover:bg-primary/8",
                  f.path === picked && "bg-primary/16 hover:bg-primary/16",
                )}
                onClick={() => setPicked(f.path)}
              >
                <span className="[overflow-wrap:anywhere]">{f.path}</span>
                <small className="whitespace-nowrap text-muted-foreground">{sizeLabel(f.size)}</small>
              </button>
            </li>
          ))}
        </ul>
      )}
      {file && <Preview item={item} file={file} onUnauthorized={onUnauthorized} />}
    </div>
  );
}
