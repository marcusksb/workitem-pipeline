import { useState } from "react";
import { Action, prNumber, searchActions, searchItems, stageLabel, ticket } from "./boardState";
import { Dialog } from "./Dialog";
import { Item } from "./types";
import { Badge } from "@/components/ui/badge";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command";

const LIMIT = 20;

type Props = { items: Item[]; actions: Action[]; showWorkspace: boolean; onOpen: (item: Item) => void; onClose: () => void };

/** Finds a workitem by title, issue key or PR number, or runs an action. Arrows pick a result, Enter opens or runs it.
 * Workitems come first, then the actions; `searchItems` and `searchActions` match and order, cmdk's own filter is off. */
export function QuickSearch({ items, actions, showWorkspace, onOpen, onClose }: Props) {
  const [query, setQuery] = useState("");
  const found = searchItems(items, query).slice(0, LIMIT);
  const foundActions = searchActions(actions, query);

  return (
    <Dialog title="Search or run" onClose={onClose} className="max-w-[520px] gap-0 p-0 [&>[data-slot=dialog-header]]:sr-only">
      <Command shouldFilter={false}>
        <CommandInput autoFocus placeholder="Workitem, action or workspace" aria-label="Search workitems" className="pr-8" value={query} onValueChange={setQuery} />
        <CommandList className="p-1">
          {query.trim() && <CommandEmpty>No match.</CommandEmpty>}
          {found.map((item) => {
            const pr = prNumber(item.pr);
            const key = ticket(item.issue)?.key;
            return (
              <CommandItem key={`${item.workspace}/${item.dir}`} value={`${item.workspace}/${item.dir}`} onSelect={() => onOpen(item)}>
                <span className="text-[16px]">{item.title}</span>
                <span className="flex flex-wrap gap-1">
                  {showWorkspace && <Badge>{item.workspace}</Badge>}
                  <Badge>{stageLabel(item.stage)}</Badge>
                  {key && <Badge>{key}</Badge>}
                  {pr && <Badge>PR #{pr}</Badge>}
                </span>
              </CommandItem>
            );
          })}
          {foundActions.length > 0 && (
            <CommandGroup heading="Actions">
              {foundActions.map((action) => (
                <CommandItem
                  key={action.label}
                  value={`action:${action.label}`}
                  onSelect={() => {
                    onClose();
                    action.run();
                  }}
                >
                  <span className="font-semibold">{action.label}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          )}
        </CommandList>
      </Command>
    </Dialog>
  );
}
