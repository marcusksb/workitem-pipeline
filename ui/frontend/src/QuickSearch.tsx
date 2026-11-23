import { useState } from "react";
import { prNumber, searchItems, stageLabel, ticket } from "./boardState";
import { Dialog } from "./Dialog";
import { Item } from "./types";
import { Badge } from "@/components/ui/badge";
import { Command, CommandEmpty, CommandInput, CommandItem, CommandList } from "@/components/ui/command";

const LIMIT = 20;

type Props = { items: Item[]; showWorkspace: boolean; onOpen: (item: Item) => void; onClose: () => void };

/** Finds a workitem by title, issue key or PR number. Arrows pick a result, Enter opens it.
 * `searchItems` matches and orders; cmdk's own filter is off. */
export function QuickSearch({ items, showWorkspace, onOpen, onClose }: Props) {
  const [query, setQuery] = useState("");
  const found = searchItems(items, query).slice(0, LIMIT);

  return (
    <Dialog title="Find workitem" onClose={onClose} className="max-w-[520px] gap-0 p-0 [&>[data-slot=dialog-header]]:sr-only">
      <Command shouldFilter={false}>
        <CommandInput autoFocus placeholder="Title, issue key or PR number" aria-label="Search workitems" className="pr-8" value={query} onValueChange={setQuery} />
        {query.trim() && (
          <CommandList className="p-1">
            <CommandEmpty>No matching workitem.</CommandEmpty>
            {found.map((item) => {
              const pr = prNumber(item.pr);
              const key = ticket(item.issue)?.key;
              return (
                <CommandItem key={`${item.workspace}/${item.dir}`} value={`${item.workspace}/${item.dir}`} onSelect={() => onOpen(item)}>
                  <span className="text-[14px]">{item.title}</span>
                  <span className="flex flex-wrap gap-1">
                    {showWorkspace && <Badge>{item.workspace}</Badge>}
                    <Badge>{stageLabel(item.stage)}</Badge>
                    {key && <Badge>{key}</Badge>}
                    {pr && <Badge>PR #{pr}</Badge>}
                  </span>
                </CommandItem>
              );
            })}
          </CommandList>
        )}
      </Command>
    </Dialog>
  );
}
