import { Item } from "./types";
import { IconExternal } from "./icons";
import { ActionBar } from "./parts";
import { Button } from "@/components/ui/button";

/** The link to the PR at the top of the PR tab. Merge and rebase are stage actions (decision 86). */
export function PrActions({ item }: { item: Item }) {
  return (
    <ActionBar>
      <Button asChild>
        <a className="pr-link no-underline" href={item.pr} target="_blank" rel="noreferrer">
          <IconExternal />
          View PR
        </a>
      </Button>
    </ActionBar>
  );
}
