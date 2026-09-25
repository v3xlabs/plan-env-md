import { For, Match, Switch } from "solid-js";

import type { DocumentSummary } from "../api/documents";
import { LIST } from "./Control";
import { DocumentCard } from "./DocumentCard";
import { DocumentRow } from "./DocumentRow";
import { DocumentTile } from "./DocumentTile";
import type { View } from "./ViewToggle";

type Properties = {
  documents: DocumentSummary[];
  view: View;
  showProject?: boolean;
};

/// One run of documents, laid out the way the page was asked for.
export const DocumentCollection = (properties: Properties) => (
  <Switch
    fallback={(
      <ul class="grid gap-3 sm:grid-cols-2 md:grid-cols-3">
        <For each={properties.documents}>
          {document => (
            <li class="min-w-0">
              <DocumentCard document={document} showProject={properties.showProject} />
            </li>
          )}
        </For>
      </ul>
    )}
  >
    <Match when={properties.view === "list"}>
      <ul class={LIST}>
        <For each={properties.documents}>
          {document => <DocumentRow document={document} showProject={properties.showProject} />}
        </For>
      </ul>
    </Match>

    {/* One surface, with a one pixel gap over the hairline colour drawing every
        separator. A last odd tile spans both columns so no empty cell shows
        the hairline colour through. */}
    <Match when={properties.view === "tiles"}>
      <ul class="grid gap-px overflow-hidden rounded-panel bg-hairline sm:grid-cols-2">
        <For each={properties.documents}>
          {document => (
            <li class="min-w-0 bg-surface sm:odd:last:col-span-2">
              <DocumentTile document={document} showProject={properties.showProject} />
            </li>
          )}
        </For>
      </ul>
    </Match>
  </Switch>
);
