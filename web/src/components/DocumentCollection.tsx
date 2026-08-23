import { For, Match, Switch } from "solid-js";

import type { DocumentSummary } from "../api/documents";
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
      <ul class="divide-y divide-line border-y border-line">
        <For each={properties.documents}>
          {document => <DocumentRow document={document} showProject={properties.showProject} />}
        </For>
      </ul>
    </Match>

    {/* A one pixel gap over the line colour draws every separator, so the
        tiles keep the list's hairlines without a border on each one. */}
    <Match when={properties.view === "tiles"}>
      <ul class="grid gap-px border-y border-line bg-line sm:grid-cols-2">
        <For each={properties.documents}>
          {document => (
            <li class="min-w-0 bg-bg">
              <DocumentTile document={document} showProject={properties.showProject} />
            </li>
          )}
        </For>
      </ul>
    </Match>
  </Switch>
);
